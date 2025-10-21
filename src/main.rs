use anyhow::{Context, Result};
use clap::{Arg, Command};
use nix::unistd::{getgid, getuid, setgid, setuid, Gid, Uid};
use parking_lot::Mutex;
use std::env;
use std::sync::mpsc;
use tokio::sync::mpsc as tokio_mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info};

mod audio_control;
mod audio_input;
mod dbus_service;
mod input_event;
mod stt_client;
mod tray_icon;
mod virtual_keyboard;
mod whisper_client;

use audio_control::AudioControl;
use audio_input::AudioInput;
use stt_client::{AudioBuffer, SttClient};
use virtual_keyboard::{RealKeyboardHardware, VirtualKeyboard};
use whisper_client::WhisperClient;

#[derive(Debug, Clone, Copy, PartialEq)]
enum SttProvider {
    WebSocket,  // Deepgram or similar WebSocket-based STT
    Rest,       // OpenAI Whisper or similar REST-based STT
}

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
            Arg::new("stt-provider")
                .long("stt-provider")
                .help("STT provider type: 'websocket' (Deepgram) or 'rest' (OpenAI Whisper)")
                .value_name("PROVIDER")
                .default_value("websocket"),
        )
        .arg(
            Arg::new("stt-url")
                .long("stt-url")
                .help("Custom STT service URL")
                .value_name("URL"),
        )
        .arg(
            Arg::new("live-mode")
                .default_value("false")
                .long("live-mode")
                .help("Type text immediately as it's transcribed (default: wait until end of turn)")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("eager-eot-threshold")
                .long("eager-eot-threshold")
                .help("Eager end-of-turn threshold (0.3-0.9, omit to disable)")
                .value_name("THRESHOLD"),
        )
        .arg(
            Arg::new("eot-threshold")
                .long("eot-threshold")
                .help("Standard end-of-turn threshold (0.5-0.9, default: 0.8, must be > eager-eot-threshold)")
                .value_name("THRESHOLD"),
        )
        .arg(
            Arg::new("save-audio")
                .long("save-audio")
                .help("Save audio to a WAV file")
                .value_name("FILE_PATH"),
        )
        .arg(
            Arg::new("inactivity-timeout")
                .long("inactivity-timeout")
                .help("Auto-toggle off after this many seconds of silence (default: 30)")
                .value_name("SECONDS")
                .default_value("30"),
        )
        .get_matches();

    // Parse and validate thresholds from command line BEFORE creating keyboard
    let eager_eot_threshold = matches
        .get_one::<String>("eager-eot-threshold")
        .and_then(|s| s.parse::<f64>().ok()); // None if not specified (disabled)

    let eot_threshold = matches
        .get_one::<String>("eot-threshold")
        .and_then(|s| s.parse::<f64>().ok())
        .or(Some(0.8)); // Default to 0.8 if not specified

    // Validate thresholds according to Deepgram API specs
    if let Some(eager) = eager_eot_threshold {
        if eager < 0.3 || eager > 0.9 {
            error!("Error: eager-eot-threshold must be between 0.3 and 0.9 (got {})", eager);
            std::process::exit(1);
        }
    }

    if let Some(standard) = eot_threshold {
        if standard < 0.5 || standard > 0.9 {
            error!("Error: eot-threshold must be between 0.5 and 0.9 (got {})", standard);
            std::process::exit(1);
        }
    }

    if let (Some(eager), Some(standard)) = (eager_eot_threshold, eot_threshold) {
        if eager >= standard {
            error!("Error: eager-eot-threshold ({}) must be less than eot-threshold ({})", eager, standard);
            error!("The eager threshold should trigger faster (lower value) than the standard threshold.");
            std::process::exit(1);
        }
    }

    // Parse inactivity timeout
    let inactivity_timeout = matches
        .get_one::<String>("inactivity-timeout")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);

    // Parse STT provider
    let stt_provider = match matches.get_one::<String>("stt-provider").map(|s| s.as_str()) {
        Some("websocket") => SttProvider::WebSocket,
        Some("rest") => SttProvider::Rest,
        Some(provider) => {
            error!("Invalid STT provider: {}. Must be 'websocket' or 'rest'", provider);
            std::process::exit(1);
        }
        None => SttProvider::WebSocket, // Default
    };

    let device_name = "Voice Keyboard";
    let delay_input = !matches.get_flag("live-mode");

    // Step 1: Create virtual keyboard while we have root privileges
    debug!("Creating virtual keyboard device (requires root privileges)...");
    let hardware =
        RealKeyboardHardware::new(device_name).context("Failed to create keyboard hardware")?;
    let keyboard = VirtualKeyboard::new(hardware, delay_input);
    debug!("Virtual keyboard created successfully");

    // Step 2: Drop root privileges before initializing audio
    original_user
        .drop_privileges()
        .context("Failed to drop root privileges")?;

    if matches.get_flag("test-audio") {
        let save_audio_path = matches.get_one::<String>("save-audio").map(|s| s.as_str());
        test_audio(save_audio_path).await?;
    } else if matches.get_flag("test-stt") {
        let stt_url = matches.get_one::<String>("stt-url");
        test_stt(keyboard, stt_provider, stt_url, eager_eot_threshold, eot_threshold, inactivity_timeout).await?;
    } else {
        let debug_mode = matches.get_flag("debug-stt");
        let stt_url = matches.get_one::<String>("stt-url");

        if debug_mode {
            debug_stt(stt_provider, stt_url, eager_eot_threshold, eot_threshold, inactivity_timeout).await?;
        } else {
            test_stt(keyboard, stt_provider, stt_url, eager_eot_threshold, eot_threshold, inactivity_timeout).await?;
        }
    }

    Ok(())
}

async fn test_audio(save_audio_path: Option<&str>) -> Result<()> {
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

    // Start saving to file if requested
    if let Some(path) = save_audio_path {
        audio_input.start_saving_to_file(path)?;
    }

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

    // Stop saving to file if it was started
    if save_audio_path.is_some() {
        audio_input.stop_saving_to_file()?;
    }

    info!("Audio test completed!");
    Ok(())
}

async fn test_stt(keyboard: VirtualKeyboard<RealKeyboardHardware>, stt_provider: SttProvider, stt_url: Option<&String>, eager_eot_threshold: Option<f64>, eot_threshold: Option<f64>, inactivity_timeout: u64) -> Result<()> {
    info!("Testing speech-to-text functionality...");

    // Wrap keyboard in a mutex to allow mutable access from the closure
    let keyboard = std::sync::Arc::new(std::sync::Mutex::new(keyboard));
    let keyboard_clone = keyboard.clone();

    run_stt(stt_provider, stt_url, eager_eot_threshold, eot_threshold, inactivity_timeout, move |result| {
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
            "EagerEndOfTurn" => {
                // Eager end of turn detected - finalize transcript
                info!("Eager end of turn detected, finalizing transcript");
                if let Err(e) = kb.finalize_transcript() {
                    error!("Failed to finalize transcript on eager EOT: {}", e);
                    std::process::exit(1);
                }
                // Mark that we've finalized so we don't do it again on EndOfTurn
                kb.mark_eager_eot_finalized();
            }
            "TurnResumed" => {
                // Turn resumed after eager EOT - reset the flag so we can finalize again
                info!("Turn resumed, continuing transcription");
                kb.reset_eager_eot_flag();
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

async fn debug_stt(stt_provider: SttProvider, stt_url: Option<&String>, eager_eot_threshold: Option<f64>, eot_threshold: Option<f64>, inactivity_timeout: u64) -> Result<()> {
    info!("Debugging speech-to-text functionality...");

    run_stt(stt_provider, stt_url, eager_eot_threshold, eot_threshold, inactivity_timeout, |result| {
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
    Cancel, // Stop recording and discard audio without transcription
}

struct ActiveSttSession {
    audio_tx: Option<tokio_mpsc::Sender<Vec<u8>>>, // For WebSocket mode
    _handle: Option<tokio::task::JoinHandle<Result<()>>>, // Kept alive to maintain the async task (WebSocket only)
    _audio_input: AudioInput, // Kept alive to maintain audio stream
    audio_buffer: Option<Arc<Mutex<Vec<u8>>>>, // For REST mode - buffer all audio data
}

async fn run_stt<F>(stt_provider: SttProvider, stt_url: Option<&String>, eager_eot_threshold: Option<f64>, eot_threshold: Option<f64>, inactivity_timeout: u64, on_transcription: F) -> Result<()>
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
    info!("STT Provider: {}", match stt_provider {
        SttProvider::WebSocket => "WebSocket (Deepgram)",
        SttProvider::Rest => "REST (OpenAI Whisper)",
    });
    if let Some(url) = stt_url {
        info!("STT URL: {}", url);
    }
    info!("Use the tray icon or D-Bus to toggle listening.");
    info!("Press Ctrl+C to quit.");
    
    // Only show EOT thresholds for WebSocket mode
    if stt_provider == SttProvider::WebSocket {
        if let Some(threshold) = eager_eot_threshold {
            info!("Eager end-of-turn threshold: {}", threshold);
        } else {
            info!("Eager end-of-turn: disabled");
        }
        if let Some(threshold) = eot_threshold {
            info!("Standard end-of-turn threshold: {}", threshold);
        }
    }
    
    info!("Auto-toggle off after {} seconds of inactivity", inactivity_timeout);

    // Shared state for STT active/inactive
    let is_active = Arc::new(Mutex::new(false));
    
    // Track last voice activity timestamp
    let last_activity = Arc::new(Mutex::new(std::time::Instant::now()));
    
    // Set up system tray (must stay on this thread)
    let mut tray_manager = tray_icon::TrayManager::new(is_active.clone())
        .context("Failed to create system tray icon")?;

    // Use channels to communicate toggle commands to STT thread
    let (cmd_tx, cmd_rx) = mpsc::channel::<SttCommand>();
    
    // Set up D-Bus service
    let dbus_service = dbus_service::DbusService::new(is_active.clone());
    let cmd_tx_dbus = cmd_tx.clone();
    dbus_service.set_toggle_callback(move |new_state| {
        info!("D-Bus toggle: {}", if new_state { "active" } else { "inactive" });
        
        // Send command to STT thread
        let cmd = if new_state { SttCommand::Start } else { SttCommand::Stop };
        let _ = cmd_tx_dbus.send(cmd);
    });
    
    let cmd_tx_cancel = cmd_tx.clone();
    dbus_service.set_cancel_callback(move || {
        info!("D-Bus cancel: cancelling recording without transcription");
        
        // Send cancel command to STT thread
        let _ = cmd_tx_cancel.send(SttCommand::Cancel);
    });
    
    // Spawn inactivity monitor thread
    let cmd_tx_inactivity = cmd_tx.clone();
    let last_activity_monitor = last_activity.clone();
    let is_active_monitor = is_active.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            
            // Only check inactivity if we're currently active
            if *is_active_monitor.lock() {
                let elapsed = last_activity_monitor.lock().elapsed();
                if elapsed >= Duration::from_secs(inactivity_timeout) {
                    info!("Inactivity timeout reached ({} seconds), auto-toggling off", inactivity_timeout);
                    // Update is_active state first to prevent repeated logs and update tray icon
                    *is_active_monitor.lock() = false;
                    // Send stop command to STT thread
                    let _ = cmd_tx_inactivity.send(SttCommand::Stop);
                }
            }
        }
    });
    
    // Spawn D-Bus service in background
    tokio::spawn(async move {
        if let Err(e) = dbus_service.start().await {
            error!("D-Bus service error: {}", e);
        }
    });
    
    // Clone necessary values for the STT thread
    let stt_url_owned = stt_url.map(|s| s.clone());
    let last_activity_clone = last_activity.clone();
    let last_activity_reset = last_activity.clone();
    
    // Wrap the transcription callback to update last activity time
    let wrapped_on_transcription = move |result: stt_client::TranscriptionResult| {
        // Update last activity time whenever we receive a non-empty transcript
        if !result.transcript.is_empty() {
            *last_activity_clone.lock() = std::time::Instant::now();
        }
        // Call the original callback
        on_transcription(result);
    };
    
    // Spawn dedicated STT management thread
    thread::spawn(move || {
        // Create a new tokio runtime for this thread
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        // Track current active session
        let mut active_session: Option<ActiveSttSession> = None;
        
        // Create audio control instance to manage system audio pause/resume
        let mut audio_control = AudioControl::new();
        
        for command in cmd_rx {
            match command {
                SttCommand::Start => {
                    // Pause system audio if playing
                    if let Err(e) = audio_control.on_recording_start() {
                        error!("Failed to control system audio: {}", e);
                    }
                    
                    // If there's an existing session, close it first
                    if let Some(session) = active_session.take() {
                        info!("Closing existing STT session...");
                        if let Some(tx) = session.audio_tx {
                            drop(tx); // This will trigger WebSocket cleanup
                        }
                        drop(session._audio_input); // Stop audio recording
                        // Don't wait for handle to finish, just move on
                    }
                    
                    // Reset inactivity timer when starting a new session
                    *last_activity_reset.lock() = std::time::Instant::now();
                    
                    // Create audio input on this thread
                    let mut audio_input = match AudioInput::new() {
                        Ok(ai) => ai,
                        Err(e) => {
                            error!("Failed to create audio input: {}", e);
                            continue;
                        }
                    };
                    
                    match stt_provider {
                        SttProvider::WebSocket => {
                            // WebSocket mode: stream audio chunks continuously
                            info!("Creating new WebSocket STT connection...");
                            let url = stt_url_owned.as_ref().map(|s| s.as_str()).unwrap_or(stt_client::STT_URL);
                            let stt_client = SttClient::with_eot_thresholds(url, sample_rate, eager_eot_threshold, eot_threshold);
                            let on_transcription_clone = wrapped_on_transcription.clone();
                            
                            match rt.block_on(stt_client.connect_and_transcribe(on_transcription_clone)) {
                                Ok((audio_tx, handle)) => {
                                    info!("STT connection established");
                                    
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
                                        audio_tx: Some(audio_tx),
                                        _handle: Some(handle),
                                        _audio_input: audio_input,
                                        audio_buffer: None,
                                    });
                                }
                                Err(e) => {
                                    error!("Failed to create STT connection: {}", e);
                                }
                            }
                        }
                        SttProvider::Rest => {
                            // REST mode: buffer all audio data
                            info!("Starting REST mode audio recording...");
                            let buffer = Arc::new(Mutex::new(Vec::new()));
                            let buffer_clone = buffer.clone();
                            
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

                                // Convert to PCM 16-bit and buffer
                                let pcm_data: Vec<u8> = mono_data
                                    .iter()
                                    .flat_map(|&sample| {
                                        let pcm_sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                                        pcm_sample.to_le_bytes()
                                    })
                                    .collect();

                                // Append to buffer
                                buffer_clone.lock().extend_from_slice(&pcm_data);
                            }) {
                                error!("Failed to start recording: {}", e);
                                continue;
                            }
                            
                            info!("Audio recording started (REST mode - buffering)");
                            
                            // Store the session with buffer
                            active_session = Some(ActiveSttSession {
                                audio_tx: None,
                                _handle: None,
                                _audio_input: audio_input,
                                audio_buffer: Some(buffer),
                            });
                        }
                    }
                }
                SttCommand::Stop => {
                    // Resume system audio if we paused it
                    if let Err(e) = audio_control.on_recording_stop() {
                        error!("Failed to control system audio: {}", e);
                    }
                    
                    // Close session: this will drop the WebSocket connection and stop audio recording
                    if let Some(session) = active_session.take() {
                        info!("Stopping STT session...");
                        
                        match stt_provider {
                            SttProvider::WebSocket => {
                                // WebSocket mode: just drop the session to clean up
                                drop(session);
                            }
                            SttProvider::Rest => {
                                // REST mode: send buffered audio to Whisper API
                                if let Some(buffer) = session.audio_buffer {
                                    // Stop recording first
                                    drop(session._audio_input);
                                    
                                    let audio_data = buffer.lock().clone();
                                    info!("Sending {} bytes of audio to Whisper API...", audio_data.len());
                                    
                                    if audio_data.is_empty() {
                                        info!("No audio data recorded, skipping transcription");
                                        continue;
                                    }
                                    
                                    // Create Whisper client and send audio
                                    let url = stt_url_owned.as_ref().map(|s| s.as_str());
                                    let whisper_client = WhisperClient::new(url);
                                    let on_transcription_clone = wrapped_on_transcription.clone();
                                    
                                    match rt.block_on(whisper_client.transcribe(&audio_data, sample_rate)) {
                                        Ok(text) => {
                                            info!("Received transcription: {}", text);
                                            
                                            // Only send transcription events if the text is not empty
                                            if !text.is_empty() {
                                                // First, send an Update event with the transcript
                                                let update_result = stt_client::TranscriptionResult {
                                                    event: "Update".to_string(),
                                                    turn_index: 0,
                                                    start: 0.0,
                                                    timestamp: 0.0,
                                                    transcript: text.clone(),
                                                    words: Vec::new(),
                                                    end_of_turn_confidence: 1.0,
                                                };
                                                on_transcription_clone(update_result);
                                                
                                                // Then, send an EndOfTurn event to finalize
                                                let eot_result = stt_client::TranscriptionResult {
                                                    event: "EndOfTurn".to_string(),
                                                    turn_index: 0,
                                                    start: 0.0,
                                                    timestamp: 0.0,
                                                    transcript: String::new(),
                                                    words: Vec::new(),
                                                    end_of_turn_confidence: 1.0,
                                                };
                                                on_transcription_clone(eot_result);
                                            } else {
                                                info!("Transcription is empty, skipping keyboard input");
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to transcribe audio: {}", e);
                                        }
                                    }
                                } else {
                                    drop(session);
                                }
                            }
                        }
                    }
                }
                SttCommand::Cancel => {
                    // Resume system audio if we paused it
                    if let Err(e) = audio_control.on_recording_stop() {
                        error!("Failed to control system audio: {}", e);
                    }
                    
                    // Cancel recording: drop the session without transcription
                    if let Some(session) = active_session.take() {
                        info!("Cancelling STT session without transcription...");
                        // Just drop everything - no transcription will occur
                        drop(session);
                    }
                }
            }
        }
    });

    // Event loop on main thread for tray events
    let mut last_state = false;
    loop {
        // Process GTK events (required for tray icon to work)
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
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

        // Check if state was changed externally (e.g., via D-Bus) and update tray icon
        let current_state = *is_active.lock();
        if current_state != last_state {
            if let Err(e) = tray_manager.update_icon(current_state) {
                error!("Failed to update tray icon: {}", e);
            }
            last_state = current_state;
        }

        thread::sleep(Duration::from_millis(100));
    }
}
