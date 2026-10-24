//! `ngambang --self-test`: checks the pieces CI can exercise without a user
//! session — config, Direct2D/DirectWrite drawing of every scene, and a pipe
//! round trip. `--strict` also creates the window and compositor.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use ngambang_core::config::{Config, ModuleId, DEFAULT_TOML};
use ngambang_core::geometry::Edge;
use ngambang_core::model::*;
use ngambang_core::protocol::{Level, Request};
use ngambang_core::view;
use windows::Win32::Foundation::HWND;

use super::gfx::{self, Gfx};
use super::util::{now_ms, Bus};

fn sample_model() -> Model {
    let now = now_ms();
    let mut m = Model::new(Config::default(), 1, now);
    m.update(
        now,
        Event::Media(vec![MediaSession {
            id: "a.exe".into(),
            app: "Player".into(),
            title: "Track".into(),
            artist: "Artist".into(),
            playing: true,
            duration_ms: 200_000,
            at_ms: now,
            can_play_pause: true,
            can_next: true,
            can_prev: true,
            can_seek: true,
            ..Default::default()
        }]),
    );
    m.update(
        now,
        Event::Mixer(MixerSnapshot {
            device: "Speakers".into(),
            master: Volume { level: 0.5, muted: false },
            apps: vec![AudioApp { pid: 9, name: "Game".into(), exe: "game.exe".into(), ..Default::default() }],
        }),
    );
    m.update(
        now,
        Event::Pipe(Request::Done {
            title: None,
            id: "cargo test".into(),
            ok: false,
            code: Some(101),
            duration_s: Some(40.0),
            hwnd: Some("0x1".into()),
        }),
    );
    m.update(now, Event::Ports(vec![PortEntry { port: 3000, pid: 42, process: "node.exe".into(), can_stop: true }]));
    m.update(now, Event::Clipboard { text: "1700000000".into(), unix_now: 1_700_000_000 });
    m.update(now, Event::Fullscreen { active: true, exe: "game.exe".into() });
    m.update(now, Event::Capture(Capture { path: "C:\\a.png".into(), name: "a.png".into(), kind: CaptureKind::Screenshot, at_ms: now }));
    m.update(now, Event::Privacy(vec![(Device::Microphone, "Voice".into())]));
    m.update(now, Event::Fullscreen { active: false, exe: String::new() });
    m
}

pub fn run(strict: bool) -> i32 {
    let mut failed = 0;
    let mut check = |name: &str, f: &mut dyn FnMut() -> Result<String, String>| {
        let t = Instant::now();
        match f() {
            Ok(detail) => println!("ok    {name:<22} {:>6.1} ms  {detail}", t.elapsed().as_secs_f64() * 1000.0),
            Err(e) => {
                println!("FAIL  {name:<22} {e}");
                failed += 1;
            }
        }
    };

    check("config", &mut || {
        let (c, w) = Config::parse(DEFAULT_TOML);
        if !w.is_empty() || c != Config::default() {
            return Err(format!("default config mismatch: {w:?}"));
        }
        Ok(format!("{} modules", ModuleId::ALL.len()))
    });

    let gfx = Gfx::new();
    check("direct2d device", &mut || {
        gfx.as_ref().map(|g| if g.warp { "WARP (software)".into() } else { "hardware".into() }).map_err(|e| e.to_string())
    });

    if let Ok(g) = &gfx {
        check("draw every scene", &mut || {
            let mut m = sample_model();
            let mut n = 0;
            for edge in Edge::ALL {
                for state in [IslandState::Seam, IslandState::Orb, IslandState::Tag, IslandState::Slips] {
                    m.state = state;
                    let slips = if state == IslandState::Slips { m.available_slips() } else { vec![ModuleId::Media] };
                    for s in slips {
                        m.front = Some(s);
                        let scene = view::build(&m, edge, g);
                        gfx::draw_offscreen(g, scene.size.w.ceil().max(1.0) as u32, scene.size.h.ceil().max(1.0) as u32, &scene.prims)
                            .map_err(|e| format!("{:?}: {e}", scene.kind))?;
                        n += 1;
                    }
                }
            }
            Ok(format!("{n} scenes"))
        });
        check("text measure", &mut || {
            use ngambang_core::view::{TextMeasure, TextStyle};
            let w = g.width("Ngambang", TextStyle::BODY);
            if w > 20.0 && w < 200.0 {
                Ok(format!("{w:.1} DIP"))
            } else {
                Err(format!("implausible width {w}"))
            }
        });
    }

    check("pipe round trip", &mut || {
        let name = format!(r"\\.\pipe\ngambang-selftest-{}", std::process::id());
        let (tx, rx) = channel();
        let accepting = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        super::pipe::serve(name.clone(), Bus::new(tx, HWND::default()), accepting).map_err(|e| e.to_string())?;
        super::pipe::send(&name, &Request::Ping.to_line()).map_err(|e| format!("{e:?}"))?;
        let req = Request::Notify {
            id: None,
            title: "self test".into(),
            body: None,
            level: Level::Ok,
            progress: None,
            sticky: false,
            ttl_ms: None,
            hwnd: None,
        };
        super::pipe::send(&name, &req.to_line()).map_err(|e| format!("{e:?}"))?;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Event::Pipe(r)) if r == req => {}
            other => return Err(format!("unexpected: {other:?}")),
        }
        match super::pipe::send(&name, "{\"v\":1,\"op\":\"nope\"}") {
            Err(super::pipe::SendError::Rejected(_)) => Ok("ping, notify, reject".into()),
            other => Err(format!("bad request accepted: {other:?}")),
        }
    });

    if strict {
        check("window + compositor", &mut || strict_window().map_err(|e| e.to_string()));
    }

    if failed == 0 {
        println!("self-test passed (ngambang {})", ngambang_core::VERSION);
        0
    } else {
        println!("self-test: {failed} check(s) failed");
        1
    }
}

fn strict_window() -> windows::core::Result<String> {
    use windows::core::w;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hinstance = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(def_proc),
            hInstance: hinstance.into(),
            lpszClassName: w!("NgambangSelfTest"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            w!("NgambangSelfTest"),
            w!("self-test"),
            WS_POPUP,
            0,
            0,
            460,
            640,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        let gfx = Gfx::new()?;
        let mut r = super::comp::Renderer::new(hwnd, &gfx)?;
        let m = sample_model();
        let scene = view::build(&m, Edge::Left, &gfx);
        r.show(&gfx, &scene, ngambang_core::geometry::Rect::new(8.0, 8.0, scene.size.w, scene.size.h), false, true)?;
        let _ = DestroyWindow(hwnd);
        Ok(format!("{:?}", scene.kind))
    }
}

unsafe extern "system" fn def_proc(
    h: HWND,
    m: u32,
    w: windows::Win32::Foundation::WPARAM,
    l: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(h, m, w, l)
}
