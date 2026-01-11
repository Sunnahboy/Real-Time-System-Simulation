

//! Live dashboard system: renders SVG waveforms + serves metrics via HTTP.
//!
//! Two parallel threads:
//! - **Render loop:** Generates SVG every 200ms (force, position, temperature, gripper, motor, stabiliser).
//! - **Web server:** HTTP listener on port 8080 serving HTML dashboard + JSON metrics + live SVG.
//!
//! Per-component deadline tracking displayed: Sensor/Processor/Actuator miss counts enable bottleneck identification.

use plotters::{
    coord::Shift,
    prelude::*,
};

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream, SocketAddr},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
    sync::atomic::{AtomicBool, Ordering},
    collections::VecDeque,
};
use log::{info, error};

use socket2::{Socket, Domain, Type, SockAddr};

use crate::utils::metrics::{SharedMetrics, MAX_POINTS};

/// Starts dashboard system: render thread + web server thread.
/// Returns: (render_handle, web_handle, shutdown_flag).
/// Shutdown_flag can be set to false to gracefully stop both threads.
pub fn start_dashboard_system(
    metrics: SharedMetrics,
) -> (thread::JoinHandle<()>, thread::JoinHandle<()>, Arc<AtomicBool>) {
    let _ = fs::create_dir_all("data/LiveDashbaord");

    let running = Arc::new(AtomicBool::new(true));
    let cached_json = Arc::new(RwLock::new(String::new()));
    let renderer_active = Arc::new(AtomicBool::new(true));

    let render_metrics = metrics.clone();
    let render_flag = running.clone();
    let cached_json_clone = cached_json.clone();
    let renderer_active_clone = renderer_active.clone();

    // Render loop: generates SVG + JSON every 200ms if data is changing
    let render_handle = thread::spawn(move || {
        const TICK_MS: u64 = 200;
        const INACTIVITY_TICKS: usize = 5;

        let mut inactivity_count: usize = 0;
        let mut last_total_cycles: u64 = 0;
        let mut first = true;

        while render_flag.load(Ordering::Relaxed) {
            // Snapshot metrics (read-only, minimal lock time)
            let (snapshot, miss_sensor, miss_processor, miss_actuator, total_cycles, last_jitter, last_latency) = {
                let m = match render_metrics.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };

                let last_jitter = m.jitter_us.back().cloned().unwrap_or(0);
                let last_latency = m.latency_us.back().cloned().unwrap_or(0);

                (
                    (
                        m.force.clone(),
                        m.position.clone(),
                        m.temperature.clone(),
                        m.gripper.clone(),
                        m.motor.clone(),
                        m.stabiliser.clone(),
                    ),
                    m.miss_sensor,
                    m.miss_processor,
                    m.miss_actuator,
                    m.total_cycles,
                    last_jitter,
                    last_latency,
                )
            };

            // Detect inactivity (no new samples): reduces CPU usage
            if first {
                last_total_cycles = total_cycles;
                first = false;
                inactivity_count = 0;
            } else if total_cycles == last_total_cycles {
                inactivity_count = inactivity_count.saturating_add(1);
            } else {
                inactivity_count = 0;
            }

            let is_active = inactivity_count < INACTIVITY_TICKS;
            renderer_active_clone.store(is_active, Ordering::Relaxed);

            if is_active {
                // Render SVG: 3x2 grid (Force, Position, Temp, Gripper, Motor, Stabiliser)
                render_svg(
                    &snapshot,
                    miss_sensor,
                    miss_processor,
                    miss_actuator,
                    total_cycles,
                    last_jitter,
                    last_latency,
                );

                // Cache JSON for web server (per-component metrics)
                let json = format!(
                    r#"{{"miss_sensor":{},"miss_processor":{},"miss_actuator":{},"total_misses":{},"cycles_observed":{},"last_jitter_us":{},"last_latency_us":{}}}"#,
                    miss_sensor,
                    miss_processor,
                    miss_actuator,
                    miss_sensor + miss_processor + miss_actuator,
                    total_cycles,
                    last_jitter,
                    last_latency
                );

                if let Ok(mut w) = cached_json_clone.write() {
                    *w = json;
                }

                thread::sleep(Duration::from_millis(TICK_MS));
            } else {
                // Inactive: sleep longer to reduce CPU
                thread::sleep(Duration::from_millis(TICK_MS * 5));
            }
        }

        info!("render_loop: exiting because running flag cleared");
    });

    let web_metrics = metrics.clone();
    let web_flag = running.clone();
    let cached_json_for_web = cached_json.clone();
    let renderer_active_for_web = renderer_active.clone();

    // Web server: HTTP listener on port 8080
    let web_handle = thread::spawn(move || {
        start_web_server_with_cache(8080, web_metrics, web_flag, cached_json_for_web, renderer_active_for_web);
    });

    (render_handle, web_handle, running)
}

/// Renders SVG dashboard: 3x2 grid of waveforms + status bar with per-component metrics.
/// Displays observational data (no verdict/color-coding); enables bottleneck identification.
fn render_svg(
    data: &(
        VecDeque<f64>,
        VecDeque<f64>,
        VecDeque<f64>,
        VecDeque<f64>,
        VecDeque<f64>,
        VecDeque<f64>,
    ),
    miss_sensor: u64,
    miss_processor: u64,
    miss_actuator: u64,
    total_cycles: u64,
    last_jitter: u64,
    last_latency: u64,
) {
    let root = SVGBackend::new("data/LiveDashbaord/dashboard_temp.svg", (1280, 950))
        .into_drawing_area();
    root.fill(&WHITE).ok();

    let (live_dashbaord_area, status_area) = root.split_vertically(850);
    let areas = live_dashbaord_area.split_evenly((3, 3));

    // Plot 6 waveforms: sensors + actuators
    plot_series(&areas[0], "Force", &data.0);
    plot_series(&areas[1], "Position", &data.1);
    plot_series(&areas[2], "Temperature", &data.2);
    plot_series(&areas[3], "Gripper", &data.3);
    plot_series(&areas[4], "Motor", &data.4);
    plot_series(&areas[5], "Stabiliser", &data.5);

    let status_font = ("sans-serif", 18).into_font().color(&BLACK);

    // Status text: per-component deadline breakdown for bottleneck identification
    let total_misses = miss_sensor + miss_processor + miss_actuator;
    let status_text = format!(
        "Misses: Total={} (Sensor={}, Processor={}, Actuator={}) | Cycles: {} | Jitter: {} μs | Latency: {} μs",
        total_misses, miss_sensor, miss_processor, miss_actuator, total_cycles, last_jitter, last_latency
    );

    status_area.draw(&Text::new(status_text, (40, 30), status_font)).ok();

    root.present().ok();

    append_metrics_comment(miss_sensor, miss_processor, miss_actuator, total_cycles, last_jitter, last_latency);

    let _ = fs::rename("data/LiveDashbaord/dashboard_temp.svg", "data/LiveDashbaord/dashboard.svg");
}

/// Appends metrics comment to SVG (observational data: miss counts, cycles, timing).
fn append_metrics_comment(
    miss_sensor: u64,
    miss_processor: u64,
    miss_actuator: u64,
    total_cycles: u64,
    last_jitter: u64,
    last_latency: u64,
) {
    if let Ok(svg) = fs::read_to_string("data/LiveDashbaord/dashboard_temp.svg") {
        let total_misses = miss_sensor + miss_processor + miss_actuator;
        let comment = format!(
            "<!-- OBSERVATIONS: total_misses={} sensor={} processor={} actuator={} cycles={} last_jitter_us={} last_latency_us={} -->",
            total_misses, miss_sensor, miss_processor, miss_actuator, total_cycles, last_jitter, last_latency,
        );

        let cleaned = if let Some(pos) = svg.find("<!-- OBSERVATIONS:") {
            svg[..pos].to_string()
        } else {
            svg
        };

        if let Some(pos) = cleaned.rfind("</svg>") {
            let mut out = cleaned.clone();
            out.insert_str(pos, &format!("\n{}\n", comment));
            let _ = fs::write("data/LiveDashbaord/dashboard_temp.svg", out);
        }
    }
}

/// Starts HTTP server on port 8080.
/// Serves: dashboard.html (GET /), dashboard.svg (GET /dashboard.svg), metrics.json (GET /metrics.json).
/// Each request spawned in separate thread; respects shutdown flag.
fn start_web_server_with_cache(
    port: u16,
    metrics: SharedMetrics,
    running: Arc<AtomicBool>,
    cached_json: Arc<RwLock<String>>,
    renderer_active: Arc<AtomicBool>,
) {
    let addr = format!("127.0.0.1:{}", port).parse::<SocketAddr>().expect("Invalid address");
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).expect("Failed to create socket");

    socket.set_reuse_address(true).ok();
    #[cfg(unix)]
    { socket.set_reuse_port(true).ok(); }

    socket.bind(&SockAddr::from(addr)).expect("Failed to bind socket (Port used)");
    socket.listen(128).expect("Failed to listen");

    let listener: TcpListener = socket.into();
    info!("Dashboard available at http://{}", addr);

    for stream in listener.incoming() {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match stream {
            Ok(mut stream) => {
                let metrics_clone = metrics.clone();
                let cached_json_clone = cached_json.clone();
                let renderer_active_clone = renderer_active.clone();

                // Spawn per-request thread (non-blocking)
                thread::spawn(move || {
                    handle_http_request_with_cache(&mut stream, metrics_clone, cached_json_clone, renderer_active_clone);
                });
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }

    info!("Web server exiting accept loop");
}

/// Handles HTTP requests: serves HTML dashboard, SVG visualization, JSON metrics.
/// Uses cached JSON if renderer is inactive (reduces lock contention).
fn handle_http_request_with_cache(
    stream: &mut TcpStream,
    metrics: SharedMetrics,
    cached_json: Arc<RwLock<String>>,
    renderer_active: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    let _ = reader.read_line(&mut line);

    let response = if line.starts_with("GET / ") {
        // Serve dashboard HTML
        match fs::read_to_string("data/LiveDashbaord/dashboard.html") {
            Ok(html) => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                html.len(),
                html
            ),
            Err(_) => "HTTP/1.1 500 Internal Server Error\r\n\r\nMissing dashboard.html".to_string(),
        }
    } else if line.contains("GET /dashboard.svg") {
        // Serve live SVG waveforms
        match fs::read_to_string("data/LiveDashbaord/dashboard.svg") {
            Ok(svg) => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\n\r\n{}",
                svg.len(),
                svg
            ),
            Err(_) => "HTTP/1.1 503 Service Unavailable\r\n\r\nDashboard not ready".to_string(),
        }
    } else if line.contains("GET /metrics.json") {
        // Serve per-component metrics (live or cached)
        if renderer_active.load(Ordering::Relaxed) {
            let m = match metrics.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };

            let last_jitter = m.jitter_us.back().cloned().unwrap_or(0);
            let last_latency = m.latency_us.back().cloned().unwrap_or(0);

            let total_misses = m.miss_sensor + m.miss_processor + m.miss_actuator;
            let json = format!(
                r#"{{"miss_sensor":{},"miss_processor":{},"miss_actuator":{},"total_misses":{},"cycles_observed":{},"last_jitter_us":{},"last_latency_us":{}}}"#,
                m.miss_sensor,
                m.miss_processor,
                m.miss_actuator,
                total_misses,
                m.total_cycles,
                last_jitter,
                last_latency
            );

            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json.len(),
                json
            )
        } else {
            // Serve cached JSON (reduces lock contention during inactivity)
            let cached = cached_json.read().map(|s| s.clone()).unwrap_or_else(|_| String::new());
            if cached.is_empty() {
                "HTTP/1.1 503 Service Unavailable\r\n\r\nMetrics not ready".to_string()
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    cached.len(),
                    cached
                )
            }
        }
    } else {
        "HTTP/1.1 404 Not Found\r\n\r\n".to_string()
    };

    let _ = stream.write_all(response.as_bytes());
}

/// Plots single waveform as line chart (X: sample index, Y: value).
fn plot_series(area: &DrawingArea<SVGBackend, Shift>, title: &str, data: &VecDeque<f64>) {
    let (min_y, max_y) = if data.is_empty() { (0.0, 1.0) } else {
        let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (min, max.max(min + 1e-6))
    };

    let mut chart = ChartBuilder::on(area).caption(title, ("sans-serif", 18)).margin(10)
        .x_label_area_size(20).y_label_area_size(40)
        .build_cartesian_2d(0..MAX_POINTS, min_y..max_y).unwrap();
    chart.configure_mesh().disable_mesh().draw().unwrap();
    chart.draw_series(LineSeries::new(data.iter().enumerate().map(|(i, v)| (i, *v)), &BLUE)).unwrap();
}

#[allow(dead_code)]
fn plot_jitter(area: &DrawingArea<SVGBackend, Shift>, title: &str, data: &VecDeque<f64>) {
    let max = data.iter().cloned().fold(0.0, f64::max).max(100.0);
    let mut chart = ChartBuilder::on(area).caption(title, ("sans-serif", 18)).margin(10)
        .x_label_area_size(20).y_label_area_size(40)
        .build_cartesian_2d(0..MAX_POINTS, 0.0..max).unwrap();
    chart.configure_mesh().disable_mesh().draw().unwrap();
    chart.draw_series(LineSeries::new(data.iter().enumerate().map(|(i, v)| (i, *v)), &RED)).unwrap();
}