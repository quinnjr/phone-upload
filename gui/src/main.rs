#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, SocketAddrV6, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

const SERVICE: &str = "_phoneupload._tcp.local.";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 560.0])
            .with_min_inner_size([320.0, 400.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Phone Upload",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

/// A phone discovered on the network.
#[derive(Clone)]
struct Phone {
    name: String,
    addrs: Vec<SocketAddr>,
}

type Phones = Arc<Mutex<HashMap<String, Phone>>>;

#[derive(Clone, PartialEq)]
enum UploadStatus {
    Uploading,
    Done,
    Failed(String),
}

struct Upload {
    name: String,
    size: u64,
    sent: Arc<AtomicU64>,
    status: Arc<Mutex<UploadStatus>>,
}

struct App {
    phones: Phones,
    uploads: Vec<Upload>,
    discovery_error: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let phones: Phones = Arc::new(Mutex::new(HashMap::new()));
        let discovery_error =
            spawn_discovery(phones.clone(), cc.egui_ctx.clone()).err().map(|e| e.to_string());
        Self {
            phones,
            uploads: Vec::new(),
            discovery_error,
        }
    }

    fn start_uploads(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let phone = {
            let phones = self.phones.lock().unwrap();
            phones.values().next().cloned()
        };
        for path in paths {
            if path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let sent = Arc::new(AtomicU64::new(0));
            let status = Arc::new(Mutex::new(match &phone {
                Some(_) => UploadStatus::Uploading,
                None => UploadStatus::Failed("no phone found".into()),
            }));
            self.uploads.insert(
                0,
                Upload {
                    name: name.clone(),
                    size,
                    sent: sent.clone(),
                    status: status.clone(),
                },
            );
            if let Some(phone) = phone.clone() {
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = put(&phone.addrs, &name, &path, &sent, &ctx);
                    *status.lock().unwrap() = match result {
                        Ok(()) => UploadStatus::Done,
                        Err(e) => UploadStatus::Failed(e.to_string()),
                    };
                    ctx.request_repaint();
                });
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.start_uploads(dropped, ctx);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        let phone = {
            let phones = self.phones.lock().unwrap();
            phones.values().next().cloned()
        };

        egui::Panel::top("status").show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(err) = &self.discovery_error {
                    ui.colored_label(ui.visuals().error_fg_color, "⚠");
                    ui.label(format!("mDNS error: {err}"));
                } else if let Some(phone) = &phone {
                    ui.colored_label(egui::Color32::from_rgb(0x2e, 0xb0, 0x6e), "●");
                    ui.label(&phone.name);
                } else {
                    ui.spinner();
                    ui.label("Searching for your phone… (is the app open?)");
                }
            });
            ui.add_space(8.0);
        });

        egui::Panel::bottom("uploads")
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                if self.uploads.is_empty() {
                    ui.weak("No uploads yet.");
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for upload in &self.uploads {
                                show_upload_row(ui, upload);
                            }
                        });
                }
                ui.add_space(6.0);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap().shrink(12.0);
            let stroke_color = if hovering {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.painter().rect_stroke(
                rect,
                12.0,
                egui::Stroke::new(if hovering { 3.0 } else { 1.5 }, stroke_color),
                egui::StrokeKind::Inside,
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(16.0)), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        let free = ui.available_height();
                        ui.add_space((free / 2.0 - 50.0).max(0.0));
                        ui.label(egui::RichText::new("⬇").size(48.0).color(stroke_color));
                        ui.label(if hovering {
                            "Release to upload"
                        } else {
                            "Drag files here to send them to your phone"
                        });
                        ui.add_space(12.0);
                        if ui.button("Choose files…").clicked() {
                            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                                self.start_uploads(paths, ctx);
                            }
                        }
                    });
                });
            });
        });
    }
}

fn show_upload_row(ui: &mut egui::Ui, upload: &Upload) {
    let status = upload.status.lock().unwrap().clone();
    ui.horizontal(|ui| {
        match &status {
            UploadStatus::Uploading => ui.spinner(),
            UploadStatus::Done => {
                ui.colored_label(egui::Color32::from_rgb(0x2e, 0xb0, 0x6e), "✔")
            }
            UploadStatus::Failed(_) => ui.colored_label(ui.visuals().error_fg_color, "✘"),
        };
        ui.label(&upload.name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(format_size(upload.size));
        });
    });
    match &status {
        UploadStatus::Uploading => {
            let sent = upload.sent.load(Ordering::Relaxed);
            let fraction = if upload.size == 0 {
                0.0
            } else {
                sent as f32 / upload.size as f32
            };
            ui.add(egui::ProgressBar::new(fraction).desired_height(6.0));
        }
        UploadStatus::Failed(err) => {
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        UploadStatus::Done => {}
    }
    ui.add_space(4.0);
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Browses for phones forever, keeping the shared map current.
fn spawn_discovery(phones: Phones, ctx: egui::Context) -> Result<(), mdns_sd::Error> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let rx = daemon.browse(SERVICE)?;
    std::thread::spawn(move || {
        // Keep the daemon alive for the lifetime of the thread.
        let _daemon = daemon;
        while let Ok(ev) = rx.recv() {
            match ev {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let addrs = socket_addrs(&info);
                    if !addrs.is_empty() {
                        let name = info
                            .fullname
                            .split('.')
                            .next()
                            .unwrap_or(&info.fullname)
                            .to_string();
                        phones
                            .lock()
                            .unwrap()
                            .insert(info.fullname.clone(), Phone { name, addrs });
                        ctx.request_repaint();
                    }
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                    phones.lock().unwrap().remove(&fullname);
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
    });
    Ok(())
}

/// All resolved addresses as connectable SocketAddrs, IPv4 first, then
/// global IPv6, then scoped link-local IPv6 as a last resort.
fn socket_addrs(info: &mdns_sd::ResolvedService) -> Vec<SocketAddr> {
    let port = info.port;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    let mut v6_ll = Vec::new();
    for scoped in &info.addresses {
        match scoped {
            mdns_sd::ScopedIp::V4(ip) => v4.push(SocketAddr::new(IpAddr::V4(*ip.addr()), port)),
            mdns_sd::ScopedIp::V6(ip) => {
                let is_link_local = (ip.addr().segments()[0] & 0xffc0) == 0xfe80;
                if is_link_local {
                    v6_ll.push(SocketAddr::V6(SocketAddrV6::new(
                        *ip.addr(),
                        port,
                        0,
                        ip.scope_id().index,
                    )));
                } else {
                    v6.push(SocketAddr::new(IpAddr::V6(*ip.addr()), port));
                }
            }
            _ => {}
        }
    }
    v4.extend(v6);
    v4.extend(v6_ll);
    v4
}

/// Streams the file as a raw PUT body, bumping `sent` as bytes go out.
/// ponytail: hand-rolled HTTP/1.1 request — no auth, no TLS, LAN only.
fn put(
    addrs: &[SocketAddr],
    name: &str,
    path: &std::path::Path,
    sent: &AtomicU64,
    ctx: &egui::Context,
) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut sock = TcpStream::connect(addrs)?;
    write!(
        sock,
        "PUT /?name={} HTTP/1.1\r\nHost: {}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        urlencode(name),
        sock.peer_addr()?,
    )?;

    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sock.write_all(&buf[..n])?;
        sent.fetch_add(n as u64, Ordering::Relaxed);
        ctx.request_repaint();
    }
    sock.flush()?;

    let mut resp = String::new();
    sock.take(256).read_to_string(&mut resp)?;
    let status = resp.lines().next().unwrap_or("");
    if !status.contains(" 200") {
        return Err(std::io::Error::other(format!("server said: {status}")));
    }
    Ok(())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_unsafe_chars() {
        assert_eq!(super::urlencode("a b/c?d.txt"), "a%20b%2Fc%3Fd.txt");
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(super::format_size(512), "512 B");
        assert_eq!(super::format_size(1536), "1.5 KB");
        assert_eq!(super::format_size(5 * 1024 * 1024), "5.0 MB");
    }
}
