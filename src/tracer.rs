use std::{collections::VecDeque, fs, path::PathBuf, process::Command};

use anyhow::{anyhow, Result};
use egui::Context;
use image::{ImageBuffer, Rgba};
use itertools::Itertools;
use tempdir::TempDir;
use tokio::sync::mpsc::Sender;

use crate::Progress;

#[derive(Clone, Debug)]
pub struct Settings {
	pub input_path: Option<PathBuf>,
	pub fps: u32,
	pub from_first: bool,
	pub threshold: u32,
	//pub denosie: u32,
}
type Image = ImageBuffer<Rgba<u8>, Vec<u8>>;
pub async fn start(set: Settings, chan: Sender<Progress>, ctx: &Context) -> Result<Image> {
	println!("{set:?}");
	let input = if let Some(input) = set.input_path {
		input
	} else {
		return Err(anyhow!("keine Eingabedatei"));
	};
	chan.send(Progress::VideoDecode).await?;
	ctx.request_repaint();
	// steps: read frames, list frame files (sorted), threshold images, subtract images
	let frames_dir = make_frames(input, set.fps)?;
	println!("frames {frames_dir:?}");
	let mut images = list_images(&frames_dir)?;
	if images.len() < 2 {
		return Err(anyhow!("zu wenig Einzelbilder"));
	}
	let mut target = read_image(
		&images
			.pop_front()
			.ok_or(anyhow!("erstes Bild konnte nicht gefunden werden"))?,
	)?;
	let mut comptarget = target.clone();
	target
		.iter_mut()
		.tuples::<(_, _, _, _)>()
		.for_each(|(_, _, _, a)| *a = 0);
	for (i, f) in images.iter().enumerate() {
		chan.send(Progress::Compare(i + 1, images.len())).await?;
		ctx.request_repaint();
		let mut img = read_image(f)?;
		let ic = if !set.from_first {
			Some(img.clone())
		} else {
			None
		};
		compare(&mut img, &comptarget, set.threshold as u8)?;
		if let Some(ic) = ic {
			comptarget = ic;
		}
		target
			.iter_mut()
			.tuples::<(_, _, _, _)>()
			.zip(img.iter().tuples::<(_, _, _, _)>())
			.for_each(|(t, s)| {
				if *s.3 > 1 && *t.3 < 1 {
					*t.0 = *s.0;
					*t.1 = *s.1;
					*t.2 = *s.2;
					*t.3 = 255;
				}
			});
	}
	target
		.iter_mut()
		.tuples::<(_, _, _, _)>()
		.for_each(|(_, _, _, a)| *a = 255);
	chan.send(Progress::Finish).await?;
	ctx.request_repaint();
	Ok(target)
}

fn compare(a: &mut Image, b: &Image, threshold: u8) -> Result<()> {
	a.iter_mut() // tuples r g b a
		.tuples::<(_, _, _, _)>()
		.zip(b.iter().tuples::<(_, _, _, _)>())
		.for_each(|(a, b)| {
			let rv = if *a.0 > *b.0 { *a.0 - b.0 } else { b.0 - *a.0 };
			let gv = if *a.1 > *b.1 { *a.1 - b.1 } else { b.1 - *a.1 };
			let bv = if *a.2 > *b.2 { *a.2 - b.2 } else { b.2 - *a.2 };
			*a.3 = if rv > threshold || gv > threshold || bv > threshold {
				255
			} else {
				0
			};
		});
	Ok(())
}

fn make_frames(input: PathBuf, fps: u32) -> Result<TempDir> {
	let tmpdir = TempDir::new("trace_a_thing_frames")?;
	let code = Command::new(if cfg!(target_is = "windows") {
		"ffmpeg.exe"
	} else {
		"ffmpeg"
	})
	.arg("-i")
	.arg(input)
	.arg("-filter:v")
	.arg(format!("fps={}", fps))
	.arg(tmpdir.path().join("%d.png"))
	.status()?;
	if !code.success() {
		return Err(anyhow!("FFMPEG Fehlercode {code}"));
	}
	Ok(tmpdir)
}

fn list_images(td: &TempDir) -> Result<VecDeque<PathBuf>> {
	let mut res: Vec<(PathBuf, usize)> = fs::read_dir(td.path())?
		.into_iter()
		.map(|x| -> Result<_> {
			let x = x?;
			let p = x.path();
			let n: usize = p
				.file_stem()
				.ok_or(anyhow!("Dateiname konnte nicht gelesen werden"))?
				.to_str()
				.ok_or(anyhow!("Dateiname konnte nicht zu Text konvertiert werden"))?
				.parse()?;
			Ok((p, n))
		})
		.collect::<Result<_>>()?;
	res.sort_by(|a, b| a.1.cmp(&b.1));
	Ok(res.into_iter().map(|x| x.0).collect())
}

fn read_image(file: &PathBuf) -> Result<Image> {
	let img = image::io::Reader::open(file)?.decode()?;
	let rgb = img.into_rgba8();
	Ok(rgb)
}
