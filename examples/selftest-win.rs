//! Windows self-test: opens a window with WebView2 the way Tauri does (wry), runs a script in the
//! page, and prints what engine answered. Started by `webview-runtime run -- selftest-win.exe`,
//! the engine is ours: the user agent then names the Fixed Version's own version.
//!
//!   cargo build --release --example selftest-win
//!   webview-runtime run -- target\release\examples\selftest-win.exe
//!
//! Exit 0 when the page's script ran and reported back, 3 when the window or engine failed,
//! 4 when nothing came back within 60 s.

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};
    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    #[derive(Debug)]
    enum Msg {
        Page(String),
    }

    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = match WindowBuilder::new().with_title("webview-runtime self-test").build(&event_loop) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("selftest-win: no window: {e}");
            std::process::exit(3);
        }
    };
    let data = std::env::temp_dir().join(format!("wvr-selftest-{}", std::process::id()));
    let mut context = wry::WebContext::new(Some(data.clone()));
    let page = r#"<!doctype html><body style="margin:0;background:#f00">
        <script>
          document.body.style.background = 'rgb(0, 255, 0)';
          const green = getComputedStyle(document.body).backgroundColor;
          window.ipc.postMessage(JSON.stringify({ green }));
        </script></body>"#;
    let built = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(page)
        .with_ipc_handler(move |req| {
            let _ = proxy.send_event(Msg::Page(req.body().clone()));
        })
        .build(&window);
    let _webview = match built {
        Ok(w) => w,
        Err(e) => {
            eprintln!("selftest-win: WebView2 did not start: {e}");
            std::process::exit(3);
        }
    };
    let engine = std::env::var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").unwrap_or_else(|_| "the system's (Evergreen)".into());
    println!("selftest-win: engine folder: {engine}");
    // the user agent says 158.0.0.0 for every 158; the loader names the exact build it picked,
    // and it honours WEBVIEW2_BROWSER_EXECUTABLE_FOLDER as the window does
    match wry::webview_version() {
        Ok(v) => println!("selftest-win: engine version: {v}"),
        Err(e) => println!("selftest-win: engine version unknown: {e}"),
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(deadline);
        match event {
            Event::UserEvent(Msg::Page(body)) => {
                println!("selftest-win: page says {body}");
                let ok = body.contains("rgb(0, 255, 0)");
                let _ = std::fs::remove_dir_all(&data);
                std::process::exit(if ok { 0 } else { 3 });
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                eprintln!("selftest-win: nothing from the page in 60 s");
                std::process::exit(4);
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => std::process::exit(3),
            _ => {}
        }
    });
}

#[cfg(not(windows))]
fn main() {
    eprintln!("selftest-win is for Windows; on Linux run `webview-runtime selftest`");
}
