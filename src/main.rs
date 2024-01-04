use std::{process, time::Duration};

use anyhow::Result;
use egui::{Button, Checkbox, Context, ProgressBar, Slider};
use tokio::runtime::Runtime;

mod tracer;

use tracer::Settings;

fn main() -> Result<()> {
	env_logger::init();
	let rt = Runtime::new()?;
	let _enter = rt.enter();
	std::thread::spawn(move || {
		rt.block_on(async {
			loop {
				tokio::time::sleep(Duration::from_secs(3600)).await
			}
		})
	}); // force rt onto seperate thread
	println!("Hello, world!");
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([320.0, 240.0])
			.with_drag_and_drop(true),
		..Default::default()
	};
	let res = eframe::run_native("TraceAThing", options, Box::new(|_| Box::<App>::default()));
	match res {
		Err(e) => panic!("error {e:?}"),
		Ok(()) => {}
	}
	Ok(())
}

struct App {
	started: bool,
	progess: f32,
	progress_text: String,
	settings: Settings,
	progress_rx: Option<tokio::sync::mpsc::Receiver<Progress>>,
	error: Option<String>,
	error_chan: Option<tokio::sync::oneshot::Receiver<String>>,
}

#[derive(Debug)]
pub enum Progress {
	VideoDecode,
	Compare(usize, usize),
	Finish,
}

impl Default for App {
	fn default() -> Self {
		let set = Settings {
			input_path: None,
			fps: 15,
			threshold: 40,
			//denosie: 2,
			from_first: false,
		};
		Self {
			error: None,
			error_chan: None,
			settings: set,
			started: false,
			progess: 0.0,
			progress_rx: None,
			progress_text: "".to_string(),
		}
	}
}
impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		ctx.set_zoom_factor(2.0);
		egui::CentralPanel::default().show(ctx, |ui| {
			if let Some(ref mut ec) = self.error_chan {
				if let Ok(e) = ec.try_recv() {
					self.error = Some(e);
				}
			}
			if let Some(e) = &self.error {
				ui.label(format!(
					"Fehler: {e}\nbitte starte Sie das Programm neu und versuchen Sie es erneut"
				));
				return;
			}
			let select_btn = Button::new(if let Some(f) = &self.settings.input_path {
				if let Some(fna) = f.file_name() {
					format!("Video: {}", fna.to_str().unwrap_or("FEHLER"))
				} else {
					format!("Video ausgewählt")
				}
			} else {
				format!("Video öffnen")
			});
			if ui.add_enabled(!self.started, select_btn).clicked() {
				let f = rfd::FileDialog::new().pick_file();
				if let Some(f) = f {
					self.settings.input_path = Some(f);
				}
			}
			ui.add_enabled(
				!self.started,
				Slider::new(&mut self.settings.fps, 1..=60).text("verwendete Bilder pro Sekunde"),
			);
			ui.add_enabled(
				!self.started,
				Slider::new(&mut self.settings.threshold, 0..=255)
					.text("Änderungsgrenzwert (0-255)"),
			);
			//ui.add_enabled(
			//	!self.started,
			//	Slider::new(&mut self.settings.denosie, 0..=5).text("% Rauschentfernung"),
			//);
			ui.add_enabled(
				!self.started,
				Checkbox::new(
					&mut self.settings.from_first,
					"verwende ersten Frame als Referenzprunkt",
				),
			);
			if !self.started {
				if ui
					.add_enabled(self.settings.input_path.is_some(), Button::new("Start"))
					.clicked()
				{
					self.started = true;
					let set = self.settings.clone();
					let (tx, rx) = tokio::sync::mpsc::channel(32);
					self.progress_rx = Some(rx);
					let (etx, erx) = tokio::sync::oneshot::channel();
					self.error_chan = Some(erx);
					let ctx = ctx.clone();
					tokio::spawn(async move {
						let err = run(set, tx, &ctx).await;
						match err {
							Ok(()) => {}
							Err(e) => {
								etx.send(format!("{e:?}"))
									.expect("error: cannot send error");
							}
						}
					});
				}
			}
			if self.started {
				if let Some(ref mut rx) = self.progress_rx {
					match rx.try_recv() {
						Ok(v) => match v {
							Progress::VideoDecode => {
								self.progress_text = "konvertiere video".to_string();
								self.progess = 0.1;
							}
							Progress::Compare(n, t) => {
								self.progress_text = format!("vergleiche Frame {n} von {t}");
								self.progess = n as f32 / t as f32;
							}
							Progress::Finish => {
								self.progress_text = "Fertig".to_string();
								self.progess = 1.0;
							}
						},
						Err(e) => match e {
							tokio::sync::mpsc::error::TryRecvError::Empty => {}
							tokio::sync::mpsc::error::TryRecvError::Disconnected => {
								self.progress_rx = None
							}
						},
					}
				}
				let bar = ProgressBar::new(self.progess).text(&self.progress_text);
				ui.add(bar);
			}
		});
	}
}
async fn run(set: Settings, tx: tokio::sync::mpsc::Sender<Progress>, ctx: &Context) -> Result<()> {
	let img = tracer::start(set, tx, &ctx).await?;
	let f = rfd::FileDialog::new()
		.set_file_name("Stroboskopbild.png")
		.add_filter("PNG Bild", &["png"])
		.save_file();
	if let Some(f) = f {
		img.save(f)?;
	}
	process::exit(0); // we're done here
	              //Ok(())
}
