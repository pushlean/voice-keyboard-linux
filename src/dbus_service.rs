use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::sync::Arc;
use tracing::info;
use zbus::{interface, ConnectionBuilder};

/// D-Bus interface for Voice Keyboard control
pub struct VoiceKeyboardInterface {
    is_active: Arc<Mutex<bool>>,
    toggle_callback: Arc<Mutex<Option<Box<dyn Fn(bool) + Send + Sync>>>>,
    cancel_callback: Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>,
}

#[interface(name = "com.voicekeyboard.Control")]
impl VoiceKeyboardInterface {
    /// Toggle the STT on/off
    async fn toggle(&mut self) -> bool {
        let mut active = self.is_active.lock();
        *active = !*active;
        let new_state = *active;
        drop(active);

        info!("D-Bus toggle: {}", if new_state { "active" } else { "inactive" });

        // Call the toggle callback if set
        if let Some(callback) = self.toggle_callback.lock().as_ref() {
            callback(new_state);
        }

        new_state
    }

    /// Get the current STT state
    async fn is_active(&self) -> bool {
        *self.is_active.lock()
    }

    /// Set STT state explicitly
    async fn set_active(&mut self, active: bool) -> bool {
        let mut current = self.is_active.lock();
        if *current != active {
            *current = active;
            drop(current);

            info!("D-Bus set_active: {}", if active { "active" } else { "inactive" });

            // Call the toggle callback if set
            if let Some(callback) = self.toggle_callback.lock().as_ref() {
                callback(active);
            }
        }
        active
    }

    /// Cancel the current recording without transcription
    async fn cancel(&mut self) -> bool {
        let mut active = self.is_active.lock();
        let was_active = *active;
        
        if was_active {
            *active = false;
            drop(active);

            info!("D-Bus cancel: stopping without transcription");

            // Call the cancel callback if set
            if let Some(callback) = self.cancel_callback.lock().as_ref() {
                callback();
            }
        }

        was_active
    }
}

/// D-Bus service manager for Voice Keyboard
pub struct DbusService {
    is_active: Arc<Mutex<bool>>,
    toggle_callback: Arc<Mutex<Option<Box<dyn Fn(bool) + Send + Sync>>>>,
    cancel_callback: Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>,
}

impl DbusService {
    pub fn new(is_active: Arc<Mutex<bool>>) -> Self {
        Self {
            is_active,
            toggle_callback: Arc::new(Mutex::new(None)),
            cancel_callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the callback that will be called when toggle is triggered via D-Bus
    pub fn set_toggle_callback<F>(&self, callback: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        *self.toggle_callback.lock() = Some(Box::new(callback));
    }

    /// Set the callback that will be called when cancel is triggered via D-Bus
    pub fn set_cancel_callback<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.cancel_callback.lock() = Some(Box::new(callback));
    }

    /// Start the D-Bus service (runs async)
    pub async fn start(self) -> Result<()> {
        let interface = VoiceKeyboardInterface {
            is_active: self.is_active.clone(),
            toggle_callback: self.toggle_callback.clone(),
            cancel_callback: self.cancel_callback.clone(),
        };

        let _connection = ConnectionBuilder::session()?
            .name("com.voicekeyboard.App")?
            .serve_at("/com/voicekeyboard/Control", interface)?
            .build()
            .await
            .context("Failed to create D-Bus connection")?;

        info!("D-Bus service started at com.voicekeyboard.App");
        info!("Available D-Bus commands:");
        info!("  Toggle: dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle");
        info!("  Cancel: dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Cancel");

        // Keep the connection alive
        std::future::pending::<()>().await;

        Ok(())
    }
}

