//! `--record`: the setup of `--screenshot`, then one frame per tick (or per `--speed` ticks),
//! sent to ffmpeg as raw pixels. The camera can move (`--pan`, `--zoom-to`, `--follow`), and the
//! video can pour materials (`--pour`), make explosions (`--boom`) and drive the robot
//! (`--drive`).

use super::{NormalView, Scene, UiState, robot_center, view_rect};
use crate::args::{Args, DriveStep};
use crate::factory_host::{FactoryCommand, PlayerInput};
use crate::normal::NormalMode;
use crate::player::MoveInput;
use crate::render_setup;
use crate::robot_sprite::{ToolKind, ToolUse};
use anyhow::{Context, Result, bail};
use foundry_content::Content;
use foundry_core::{CellPos, Command, PaintMode};
use foundry_render::headless::{capture, capture_with, create_device};
use foundry_render::wgpu;
use glam::DVec2;
use std::io::Write;
use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::sync::Arc;
use std::time::Instant;

/// Frames per second of the video.
const FPS: u32 = 60;

pub fn run(args: &Args, out: &Path, content: Arc<Content>) -> Result<()> {
    let start = Instant::now();
    let rec = &args.rec;
    let mut scene = Scene::prepare(args, content.clone())?;
    let size = scene.size;
    let frames = (rec.seconds * FPS as f64).round().max(1.0) as u32;
    // --pour and --boom places are relative to the start center.
    let origin = scene.center;
    let pours = rec
        .pours
        .iter()
        .map(|p| Ok((p, content.material(&p.material).with_context(|| format!("--pour: unknown material `{}`", p.material))?)))
        .collect::<Result<Vec<_>>>()?;
    if !rec.drive.is_empty() && scene.host.is_none() {
        bail!("--drive needs the normal mode");
    }

    let (device, queue) = create_device().context("no GPU adapter found")?;
    let mut renderer = scene.renderer(&device, &queue, &content);
    let look = scene.host.is_some().then(|| render_setup::load_robot_look(&mut renderer));
    let first_snapshot = scene.sim.take_snapshot();
    let mut panel = (!args.no_ui).then(|| {
        let camera = scene.camera();
        let mut p = scene.make_ui(args, &device, &content, &camera, &first_snapshot);
        p.hud = !rec.no_hud;
        p
    });
    if let Some(p) = panel.as_mut() {
        let stats = scene.stats(&first_snapshot, &renderer, &scene.camera());
        p.layout(&device, &queue, &stats, scene.state == UiState::Debug, 6);
    }
    let first_camera = scene.camera();
    renderer.apply_snapshot(&first_snapshot, &first_camera);
    let mut normal = NormalMode::new();

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot make folder {}", dir.display()))?;
    }
    let mut ffmpeg = Process::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
        .arg(format!("{}x{}", size.0, size.1))
        .args(["-r", &FPS.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(out)
        .stdin(Stdio::piped())
        .spawn()
        .context("cannot start ffmpeg (is it installed?)")?;
    let mut pipe = ffmpeg.stdin.take().context("no ffmpeg input")?;

    let start_zoom = scene.zoom;
    let mut step: Option<usize> = None;
    // --follow starts at the robot (a screen setup can move it after the start center was set).
    let mut look_at = match (&scene.host, rec.follow) {
        (Some(h), true) => robot_center(h),
        _ => scene.center,
    };
    for f in 0..frames {
        let t = f as f64 / FPS as f64;
        // --drive: the keys of this time.
        let now = drive_step(&rec.drive, t);
        if let Some(h) = scene.host.as_mut()
            && !rec.drive.is_empty()
            && (now != step || rec.drive[now.unwrap_or(0)].dig.is_some())
        {
            step = now;
            let keys = now.map(|i| rec.drive[i].clone()).unwrap_or_default();
            let aim = keys.dig.is_some().then(|| dig_aim(h, &keys));
            let input = PlayerInput {
                movement: MoveInput { x: keys.x, jump: keys.jump },
                aim: aim.unwrap_or_default(),
                dig: aim.is_some(),
                view: view_rect(scene.center, scene.zoom, size),
                ..Default::default()
            };
            h.apply(FactoryCommand::Input(input), &mut scene.sim);
        }
        for _ in 0..rec.speed {
            for (p, material) in &pours {
                if (p.from..p.to).contains(&t) {
                    let center = CellPos::new(origin.x as i32 + p.at.0, origin.y as i32 + p.at.1);
                    scene.sim.apply(Command::Paint { center, radius: p.radius, material: *material, mode: PaintMode::OnlyAir, temperature: None });
                }
            }
            scene.sim.tick();
            if let Some(h) = scene.host.as_mut() {
                h.tick(&mut scene.sim);
            }
        }
        for b in &rec.booms {
            if (b.time * FPS as f64).round() as u32 == f {
                let center = CellPos::new(origin.x as i32 + b.at.0, origin.y as i32 + b.at.1);
                scene.sim.apply(Command::Explode { center, strength: b.strength, heat: b.heat });
            }
        }

        // The camera.
        let k = if frames > 1 { f as f64 / (frames - 1) as f64 } else { 0.0 };
        let ease = k * k * (3.0 - 2.0 * k);
        if let Some(z) = rec.zoom_to {
            // A smooth change, equal in ratio per time (as the eye sees zoom).
            scene.zoom = (start_zoom as f64 * (z as f64 / start_zoom as f64).powf(ease)) as f32;
        }
        scene.center = match (&scene.host, rec.follow) {
            (Some(h), true) => {
                // Follow the robot with a little delay, as a camera person would.
                look_at += (robot_center(h) - look_at) * 0.08;
                look_at
            }
            _ => origin + DVec2::new(rec.pan.0, rec.pan.1) * t,
        };
        let camera = scene.camera();
        scene.sim.apply(Command::SetView { area: view_rect(scene.center, scene.zoom, size) });
        let snapshot = scene.sim.take_snapshot();
        renderer.apply_snapshot(&snapshot, &camera);
        renderer.set_time(scene.sim.tick_count() as f64 * foundry_core::TICK_SECONDS);

        // The robot and its tool, and the factory views for the shapes and the HUD.
        let frame = scene.host.as_mut().map(|h| h.frame(&scene.sim, 0));
        let digging = now.map(|i| &rec.drive[i]).filter(|s| s.dig.is_some());
        let tool = match (&frame, &scene.host, digging) {
            (Some(fr), Some(h), Some(s)) => Some(ToolUse {
                kind: ToolKind::Dig,
                aim: crate::overlay::aim_point(fr, Some(dig_aim(h, s))),
                working: fr.digging,
                color: fr.dug_material.and_then(|m| content.materials.colors[m.index()].first().copied()),
            }),
            _ => None,
        };
        scene.robot_sprites(&mut renderer, look.as_ref(), tool);

        let pixels = match panel.as_mut() {
            None => capture(&device, &queue, &mut renderer, &camera),
            Some(p) => {
                if let Some(fr) = frame {
                    normal.take_frame(fr.clone(), Instant::now());
                    normal.fill_model(&mut p.ui.model);
                    p.ui.model.perf = None;
                    p.ui.model.settings.show_fps = false;
                    if let Some((_, nv)) = p.overlay.take() {
                        p.overlay = Some((camera, NormalView { frame: fr, build: nv.build }));
                    }
                }
                let stats = scene.stats(&snapshot, &renderer, &camera);
                p.layout(&device, &queue, &stats, scene.state == UiState::Debug, 1);
                capture_with(&device, &queue, &mut renderer, &camera, |encoder, view| p.draw(&device, &queue, encoder, view, None::<wgpu::Color>))
            }
        };
        pipe.write_all(&pixels).context("ffmpeg stopped")?;
    }
    drop(pipe);
    let status = ffmpeg.wait().context("ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg failed: {status}");
    }
    println!(
        "saved {} ({}x{}, start center {:.0},{:.0}, {} frames, {} ticks per frame, {:.1} s of video) in {:.1} s",
        out.display(),
        size.0,
        size.1,
        origin.x,
        origin.y,
        frames,
        rec.speed,
        frames as f64 / FPS as f64,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

/// The index of the `--drive` step at time `t` (seconds). `None` after the last step.
fn drive_step(steps: &[DriveStep], t: f64) -> Option<usize> {
    let mut end = 0.0;
    for (i, s) in steps.iter().enumerate() {
        end += s.seconds;
        if t < end {
            return Some(i);
        }
    }
    None
}

/// The dig place of a step, in world cells.
fn dig_aim(h: &crate::factory_host::FactoryHost, s: &DriveStep) -> CellPos {
    let (x, y) = h.robot.center();
    let (dx, dy) = s.dig.unwrap_or_default();
    CellPos::new(x as i32 + dx, y as i32 + dy)
}
