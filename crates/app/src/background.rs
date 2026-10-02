//! The Background portal: keeps Permafrost running without a window and
//! shows "Frozen until 15:30" under Background Apps in Quick Settings.
//!
//! The portal only serves sandboxed apps, so outside Flatpak these do nothing;
//! the app then stays alive through its own hold on the application.

use ashpd::desktop::background::{Background, BackgroundProxy, SetStatusOptions};

fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Asks to keep running in the background. Safe to call more than once.
pub async fn request() -> bool {
    if !sandboxed() {
        return false;
    }
    let request = Background::request()
        .reason("Show how long a freeze has left and tell you when an app is blocked")
        .auto_start(false)
        .send()
        .await;
    match request.and_then(|r| r.response()) {
        Ok(response) => response.run_in_background(),
        Err(e) => {
            eprintln!("background portal unavailable: {e}");
            false
        }
    }
}

/// Sets the line shown under the app in Background Apps. Empty clears it.
pub async fn set_status(message: &str) {
    if !sandboxed() {
        return;
    }
    let Ok(proxy) = BackgroundProxy::new().await else {
        return;
    };
    // The portal allows at most 96 characters.
    let message: String = message.chars().take(96).collect();
    if let Err(e) = proxy.set_status(SetStatusOptions::default().set_message(&message)).await {
        eprintln!("couldn't set background status: {e}");
    }
}
