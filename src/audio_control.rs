use anyhow::Result;
use mpris::{PlayerFinder, PlaybackStatus};
use tracing::{debug, info, warn};

/// Manages system audio playback control via MPRIS DBus interface
pub struct AudioControl {
    paused_players: Vec<String>,
}

impl AudioControl {
    pub fn new() -> Self {
        Self {
            paused_players: Vec::new(),
        }
    }

    /// Called when toggling recording ON - pause audio if playing
    pub fn on_recording_start(&mut self) -> Result<()> {
        // Clear any previous state
        self.paused_players.clear();
        
        // Find all active media players via MPRIS
        match PlayerFinder::new() {
            Ok(finder) => {
                match finder.find_all() {
                    Ok(players) => {
                        for player in players {
                            // Check if player is currently playing
                            if let Ok(PlaybackStatus::Playing) = player.get_playback_status() {
                                let player_name = player.identity();
                                debug!("Found playing media: {}", player_name);
                                
                                // Pause it
                                if let Err(e) = player.pause() {
                                    warn!("Failed to pause {}: {}", player_name, e);
                                } else {
                                    info!("Paused media player: {}", player_name);
                                    self.paused_players.push(player_name.to_string());
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Could not find media players: {}", e);
                    }
                }
            }
            Err(e) => {
                debug!("MPRIS not available: {}", e);
            }
        }
        
        Ok(())
    }

    /// Called when toggling recording OFF - resume audio if we paused it
    pub fn on_recording_stop(&mut self) -> Result<()> {
        // Resume any players we paused
        if !self.paused_players.is_empty() {
            match PlayerFinder::new() {
                Ok(finder) => {
                    match finder.find_all() {
                        Ok(players) => {
                            for player in players {
                                let player_name = player.identity().to_string();
                                
                                // Only resume players we paused
                                if self.paused_players.contains(&player_name) {
                                    if let Err(e) = player.play() {
                                        warn!("Failed to resume {}: {}", player_name, e);
                                    } else {
                                        info!("Resumed media player: {}", player_name);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Could not find media players: {}", e);
                        }
                    }
                }
                Err(e) => {
                    debug!("MPRIS not available: {}", e);
                }
            }
            
            self.paused_players.clear();
        }
        
        Ok(())
    }
}


