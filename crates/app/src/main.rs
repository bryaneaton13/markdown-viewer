mod actions;
mod files;
mod preferences;
mod reader;

use actions::*;
use gpui_kit::{base::Root, component::TitleBar, *};
use reader::Reader;
use std::path::PathBuf;

struct MainWindow(Option<WindowHandle<Root>>, Option<Entity<Reader>>);
impl Global for MainWindow {}

fn open(path: Option<PathBuf>, cx: &mut App) {
    let existing = cx.global::<MainWindow>();
    if let (Some(handle), Some(view)) = (existing.0, existing.1.clone())
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        if let Some(path) = path {
            view.update(cx, |v, cx| v.open_path(path, cx));
        }
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
        .with_assets(gpui_kit::assets::Assets)
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
        cx.background_executor()
            .timer(std::time::Duration::from_millis(700))
            .await;
        let result = window.update(cx, |_, window, _| window.render_to_image());
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
