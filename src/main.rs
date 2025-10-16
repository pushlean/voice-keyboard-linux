use anyhow::{Context, Result};
use clap::{Arg, Command};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, hotkey::{Code, HotKey, Modifiers}};
use nix::unistd::{getgid, getuid, setgid, setuid, Gid, Uid};
use parking_lot::Mutex;
use std::env;
use std::sync::mpsc;
use tokio::sync::mpsc as tokio_mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info};

mod audio_input;
mod input_event;
mod stt_client;
mod tray_icon;
mod virtual_keyboard;

use audio_input::AudioInput;
use stt_client::{AudioBuffer, SttClient};
use virtual_keyboard::{RealKeyboardHardware, VirtualKeyboard};

#[derive(Debug)]
struct OriginalUser {
    uid: Uid,
    gid: Gid,
    home: Option<String>,
    user: Option<String>,
}

impl OriginalUser {
    fn capture() -> Self {
        // If we're running under sudo, get the original user info
        let uid = if let Ok(sudo_uid) = env::var("SUDO_UID") {
            Uid::from_raw(sudo_uid.parse().unwrap_or_else(|_| getuid().as_raw()))
        } else {
            getuid()
        };

        let gid = if let Ok(sudo_gid) = env::var("SUDO_GID") {
            Gid::from_raw(sudo_gid.parse().unwrap_or_else(|_| getgid().as_raw()))
        } else {
            getgid()
        };

        let home = env::var("HOME").ok();
        let user = env::var("SUDO_USER").ok().or_else(|| env::var("USER").ok());

        Self {
            uid,
            gid,
            home,
            user,
        }
    }

    fn drop_privileges(&self) -> Result<()> {
        if getuid().is_root() {
            debug!(
                "Dropping root privileges to uid={}, gid={}",
                self.uid, self.gid
            );

            // Preserve important environment variables
            let pulse_runtime_path = env::var("PULSE_RUNTIME_PATH").ok();
            let xdg_runtime_dir = env::var("XDG_RUNTIME_DIR").ok();
            let display = env::var("DISPLAY").ok();
            let wayland_display = env::var("WAYLAND_DISPLAY").ok();

            // Drop group first, then user (required order)
            setgid(self.gid).context("Failed to drop group privileges")?;
            setuid(self.uid).context("Failed to drop user privileges")?;

            // Restore environment variables for the original user
            if let Some(ref home) = self.home {
                env::set_var("HOME", home);
            }
            if let Some(ref user) = self.user {
                env::set_var("USER", user);
            }

            // Restore audio-related environment variables
            if let Some(pulse_path) = pulse_runtime_path {
                env::set_var("PULSE_RUNTIME_PATH", pulse_path);
            }
            if let Some(xdg_path) = xdg_runtime_dir {
                env::set_var("XDG_RUNTIME_DIR", xdg_path);
            }
            if let Some(disp) = display {
                env::set_var("DISPLAY", disp);
            }
            if let Some(wayland_disp) = wayland_display {
                env::set_var("WAYLAND_DISPLAY", wayland_disp);
            }

            debug!("Successfully dropped privileges to user");

            // Give audio system a moment to be ready
            std::thread::sleep(std::time::Duration::from_millis(100));
        } else {
            debug!("Not running as root, no privilege dropping needed");
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Starting Voice Keyboard v{}", env!("CARGO_PKG_VERSION"));

    // Capture original user info before we do anything
    let original_user = OriginalUser::capture();

    let matches = Command::new("voice-keyboard")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Voice-controlled keyboard input")
        .arg(
            Arg::new("test-audio")
                .long("test-audio")
                .help("Test audio input")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("test-stt")
                .long("test-stt")
                .help("Test speech-to-text functionality")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("debug-stt")
                .long("debug-stt")
                .help("Debug speech-to-text (print transcripts without typing)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("stt-url")
                .long("stt-url")
                .help("Custom STT service URL")
                .value_name("URL"),
        )
        .get_matches();

    let device_name = "Voice Keyboard";

    // Step 1: Create virtual keyboard while we have root privileges
    debug!("Creating virtual keyboard device (requires root privileges)...");
    let hardware =
        RealKeyboardHardware::new(device_name).context("Failed to create keyboard hardware")?;
    let keyboard = VirtualKeyboard::new(hardware);
    debug!("Virtual keyboard created successfully");

    // Step 2: Drop root privileges before initializing audio
    original_user
        .drop_privileges()
        .context("Failed to drop root privileges")?;

    if matches.get_flag("test-audio") {
        test_audio().await?;
    } else if matches.get_flag("test-stt") {
        let stt_url = matches
            .get_one::<String>("stt-url")
            .map(|s| s.as_str())
            .unwrap_or(stt_client::STT_URL);
        test_stt(keyboard, stt_url).await?;
    } else {
        let debug_mode = matches.get_flag("debug-stt");
        let stt_url = matches
            .get_one::<String>("stt-url")
            .map(|s| s.as_str())
            .unwrap_or(stt_client::STT_URL);

        if debug_mode {
            debug_stt(stt_url).await?;
        } else {
            test_stt(keyboard, stt_url).await?;
        }
    }

    Ok(())
}

async fn test_audio() -> Result<()> {
    info!("Testing audio input...");

    // List available devices
    info!("Available input devices:");
    let devices = AudioInput::list_available_devices()?;
    for (i, device) in devices.iter().enumerate() {
        info!("  {}: {}", i + 1, device);
    }

    // Create audio input
    let mut audio_input = AudioInput::new()?;
    debug!(
        "Using audio device with {} channels at {} Hz",
        audio_input.get_channels(),
        audio_input.get_sample_rate()
    );

    // Test recording for 5 seconds
    let (tx, rx) = mpsc::channel();

    audio_input.start_recording(move |data| {
        let level = data.iter().map(|&x| x.abs()).sum::<f32>() / data.len() as f32;
        let _ = tx.send(level);
    })?;

    info!("Recording for 5 seconds...");
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(level) = rx.try_recv() {
            let bar_length = (level * 50.0) as usize;
            let bar: String = "#".repeat(bar_length);
            info!("Level: {:.2} [{}]", level, bar);
        }
        thread::sleep(Duration::from_millis(50));
    }

    info!("Audio test completed!");
    Ok(())
}

async fn test_stt(keyboard: VirtualKeyboard<RealKeyboardHardware>, stt_url: &str) -> Result<()> {
    info!("Testing speech-to-text functionality...");

    // Wrap keyboard in a mutex to allow mutable access from the closure
    let keyboard = std::sync::Arc::new(std::sync::Mutex::new(keyboard));
    let keyboard_clone = keyboard.clone();

    run_stt(stt_url, move |result| {
        if !result.transcript.is_empty() {
            info!("Transcription [{}]: {}", result.event, result.transcript);
        }

        let mut kb = keyboard_clone.lock().unwrap();

        // Handle different event types
        match result.event.as_str() {
            "EndOfTurn" => {
                // Finalize the transcript; treat failure as fatal
                if let Err(e) = kb.finalize_transcript() {
                    error!("Failed to finalize transcript: {}", e);
                    std::process::exit(1);
                }
            }
            _ => {
                // Handle incremental updates; treat failure as fatal
                if let Err(e) = kb.update_transcript(&result.transcript) {
                    error!("Failed to update transcript: {}", e);
                    std::process::exit(1);
                }
            }
        }
    })
    .await
}

async fn debug_stt(stt_url: &str) -> Result<()> {
    info!("Debugging speech-to-text functionality...");
    info!("STT Service URL: {}", stt_url);

    run_stt(stt_url, |result| {
        // Only show non-empty transcriptions
        if !result.transcript.is_empty() {
            info!("Transcription [{}]: {}", result.event, result.transcript);
        }
    })
    .await
}

enum SttCommand {
    Start,
    Stop,
}

struct ActiveSttSession {
    audio_tx: tokio_mpsc::Sender<Vec<u8>>,
    _handle: tokio::task::JoinHandle<Result<()>>, // Kept alive to maintain the async task
    _audio_input: AudioInput, // Kept alive to maintain audio stream
}

async fn run_stt<F>(stt_url: &str, on_transcription: F) -> Result<()>
where
    F: Fn(stt_client::TranscriptionResult) + Send + 'static + Clone,
{
    // Initialize GTK for tray icon
    gtk::init().context("Failed to initialize GTK")?;
    
    // Create audio input temporarily just to get parameters
    let temp_audio = AudioInput::new()?;
    let sample_rate = temp_audio.get_sample_rate();
    let channels = temp_audio.get_channels();
    debug!(
        "Using audio device with {} channels at {} Hz",
        channels,
        sample_rate
    );
    drop(temp_audio);

    info!("Voice Keyboard is ready!");
    info!("Press Super+X to toggle listening, or use the tray icon.");
    info!("Press Ctrl+C to quit.");

    // Shared state for STT active/inactive
    let is_active = Arc::new(Mutex::new(false));
    
    // Set up system tray (must stay on this thread)
    let mut tray_manager = tray_icon::TrayManager::new(is_active.clone())
        .context("Failed to create system tray icon")?;

    // Set up global hotkey (Super+M)
    let hotkey_manager = GlobalHotKeyManager::new()
        .context("Failed to initialize global hotkey manager")?;
    
    let super_m = HotKey::new(Some(Modifiers::SUPER), Code::KeyM);
    hotkey_manager.register(super_m)
        .context("Failed to register Super+M hotkey")?;
    
    info!("Registered global hotkey: Super+M");

    // Use channels to communicate toggle commands to STT thread
    let (cmd_tx, cmd_rx) = mpsc::channel::<SttCommand>();
    
    // Clone necessary values for the STT thread
    let stt_url = stt_url.to_string();
    
    // Spawn dedicated STT management thread
    thread::spawn(move || {
        // Create a new tokio runtime for this thread
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        // Track current active session
        let mut active_session: Option<ActiveSttSession> = None;
        
        for command in cmd_rx {
            match command {
                SttCommand::Start => {
                    // If there's an existing session, close it first
                    if let Some(session) = active_session.take() {
                        info!("Closing existing STT session...");
                        drop(session.audio_tx); // This will trigger WebSocket cleanup
                        drop(session._audio_input); // Stop audio recording
                        // Don't wait for handle to finish, just move on
                    }
                    
                    // Create new STT connection
                    info!("Creating new STT connection...");
                    let stt_client = SttClient::new(&stt_url, sample_rate);
                    let on_transcription_clone = on_transcription.clone();
                    
                    match rt.block_on(stt_client.connect_and_transcribe(on_transcription_clone)) {
                        Ok((audio_tx, handle)) => {
                            info!("STT connection established");
                            
                            // Create audio input on this thread
                            let mut audio_input = match AudioInput::new() {
                                Ok(ai) => ai,
                                Err(e) => {
                                    error!("Failed to create audio input: {}", e);
                                    continue;
                                }
                            };
                            
                            // Start recording
                            info!("Starting audio recording...");
                            let audio_tx_clone = audio_tx.clone();
                            let audio_buffer = Arc::new(Mutex::new(AudioBuffer::new(sample_rate, 160)));
                            
                            if let Err(e) = audio_input.start_recording(move |data| {
                                debug!("Received audio data: {} samples", data.len());

                                // Average stereo channels to mono
                                let mono_data: Vec<f32> = if channels == 2 {
                                    let mut mono = Vec::with_capacity(data.len() / 2);
                                    for chunk in data.chunks_exact(2) {
                                        mono.push((chunk[0] + chunk[1]) / 2.0);
                                    }
                                    debug!("Averaged samples: {}", mono.len());
                                    mono
                                } else {
                                    data.to_vec()
                                };

                                // Create audio chunks and send them
                                let mut buffer = audio_buffer.lock();
                                let chunks = buffer.add_samples(&mono_data);
                                for chunk in chunks {
                                    debug!("Sending audio chunk: {} bytes", chunk.len());
                                    if let Err(e) = audio_tx_clone.blocking_send(chunk) {
                                        error!("Failed to send audio chunk: {}", e);
                                    }
                                }
                            }) {
                                error!("Failed to start recording: {}", e);
                                continue;
                            }
                            
                            // Store the complete session (connection + audio input)
                            active_session = Some(ActiveSttSession {
                                audio_tx,
                                _handle: handle,
                                _audio_input: audio_input,
                            });
                        }
                        Err(e) => {
                            error!("Failed to create STT connection: {}", e);
                        }
                    }
                }
                SttCommand::Stop => {
                    // Close session: this will drop the WebSocket connection and stop audio recording
                    if let Some(session) = active_session.take() {
                        info!("Stopping STT session...");
                        drop(session); // Drops audio_tx (closes WebSocket) and _audio_input (stops recording)
                    }
                }
            }
        }
    });

    // Event loop on main thread for hotkey and tray events
    loop {
        // Process GTK events (required for tray icon to work)
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        
        // Check for hotkey events
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == super_m.id() {
                let mut active = is_active.lock();
                *active = !*active;
                let new_state = *active;
                drop(active);

                info!("Hotkey toggle: {}", if new_state { "active" } else { "inactive" });
                
                if let Err(e) = tray_manager.update_icon(new_state) {
                    error!("Failed to update tray icon: {}", e);
                }
                
                // Send command to STT thread
                let cmd = if new_state { SttCommand::Start } else { SttCommand::Stop };
                let _ = cmd_tx.send(cmd);
            }
        }

        // Check for tray menu events
        if let Ok(state_changed) = tray_manager.handle_events() {
            if state_changed {
                let new_state = *is_active.lock();
                
                info!("Tray toggle: {}", if new_state { "active" } else { "inactive" });
                
                // Send command to STT thread
                let cmd = if new_state { SttCommand::Start } else { SttCommand::Stop };
                let _ = cmd_tx.send(cmd);
            }
        }

        thread::sleep(Duration::from_millis(100));
    }
}
