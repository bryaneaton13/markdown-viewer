mod actions;
mod chrome;
mod files;
mod info;
mod names;
mod pane;
mod pet;
mod preferences;
mod reader;
mod sidebar;
mod tabs;
mod workspace;

use actions::*;
use gpui_kit::{base::Root, component::TitleBar, *};
use reader::Reader;
use std::{borrow::Cow, path::PathBuf};

// Lucide icons beyond GPUI Kit's default component set, embedded one by one.
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [ArrowDownUp, ChevronsDownUp, FolderPlus, Palette]
);

/// GPUI Kit's component icons plus `ExtraIcons`.
struct AppAssets;
impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

struct MainWindow(Option<WindowHandle<Root>>, Option<Entity<Reader>>);
impl Global for MainWindow {}

fn open(path: Option<PathBuf>, cx: &mut App) {
    let existing = cx.global::<MainWindow>();
    if let (Some(handle), Some(view)) = (existing.0, existing.1.clone())
        && handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                if let Some(path) = path.clone() {
                    view.update(cx, |v, cx| v.open_path(path, window, cx));
                }
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1120.), px(820.)), cx)),
        window_min_size: Some(size(px(680.), px(460.))),
        titlebar: Some(TitlebarOptions {
            title: Some("Markdown Viewer".into()),
            traffic_light_position: Some(point(px(16.), px(15.))),
            ..TitleBar::title_bar_options()
        }),
        focus: std::env::var_os("MARKDOWN_VIEWER_SCREENSHOT").is_none(),
        ..TitleBar::window_options()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Reader::new(path, window, cx))
    }) {
        Ok((handle, view)) => {
            cx.set_global(MainWindow(handle.downcast::<Root>(), Some(view)));
        }
        Err(e) => eprintln!("Could not open viewer: {e:#}"),
    }
}

fn main() {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let app = gpui_kit::application()
        .with_assets(AppAssets)
        .with_quit_mode(QuitMode::Explicit);
    let (tx, rx) = async_channel::unbounded();
    app.on_open_urls(move |urls| {
        let _ = tx.try_send(urls);
    });
    app.on_reopen(|cx| open(None, cx));
    app.run(move |cx| {
        gpui_kit::init(cx);
        cx.set_global(MainWindow(None, None));
        actions::bind(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.spawn(async move |cx| {
            while let Ok(urls) = rx.recv().await {
                cx.update(|cx| {
                    for url in urls {
                        if let Ok(path) = url::Url::parse(&url).and_then(|u| {
                            u.to_file_path()
                                .map_err(|_| url::ParseError::RelativeUrlWithoutBase)
                        }) {
                            open(Some(path), cx);
                        }
                    }
                });
            }
        })
        .detach();
        open(paths.first().cloned(), cx);
        #[cfg(feature = "screenshot")]
        screenshot(cx);
    });
}

#[cfg(feature = "screenshot")]
fn screenshot(cx: &mut App) {
    let Ok(out) = std::env::var("MARKDOWN_VIEWER_SCREENSHOT") else {
        return;
    };
    let Some((window, view)) = cx
        .global::<MainWindow>()
        .0
        .zip(cx.global::<MainWindow>().1.clone())
    else {
        return;
    };
    cx.spawn(async move |cx| {
        // Wait for background file loading and TextView parsing, then apply the
        // requested UI state and explicitly repaint before capturing real pixels.
        for _ in 0..100 {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            let ready = view.read_with(cx, |v, cx| v.ready(cx));
            if ready {
                break;
            }
        }
        let _ = window.update(cx, |_, window, cx| {
            view.update(cx, |v, cx| {
                v.debug_actions(
                    &std::env::var("MARKDOWN_VIEWER_ACTIONS").unwrap_or_default(),
                    window,
                    cx,
                )
            });
            window.refresh();
        });
        // Images decode and popovers measure over a few frames; a busy
        // machine may need longer (MARKDOWN_VIEWER_SHOT_DELAY, in ms).
        let settle = std::env::var("MARKDOWN_VIEWER_SHOT_DELAY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(700);
        // Draw a few frames ourselves: a window on another Space or behind a
        // full-screen app gets no display-link frames, and popovers place
        // themselves from the previous frame's layout.
        for _ in 0..4 {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(settle / 4))
                .await;
            let _ = cx.update_window(window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        }
        // Draw now rather than trusting the last frame: an occluded or
        // unattended window may not have repainted since launch.
        let result = cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            window.render_to_image()
        });
        let ok = match result {
            Ok(Ok(image)) => image.save(&out).map(|_| eprintln!("Saved {out}")).is_ok(),
            other => {
                eprintln!("Screenshot failed: {other:?}");
                false
            }
        };
        if !ok {
            std::process::exit(1);
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
