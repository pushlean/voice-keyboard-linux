use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use tracing::{debug, info};

pub struct TrayManager {
    _tray_icon: TrayIcon,
    toggle_item: MenuItem,
    is_active: Arc<Mutex<bool>>,
}

impl TrayManager {
    pub fn new(is_active: Arc<Mutex<bool>>) -> Result<Self> {
        // Create menu items
        let toggle_item = MenuItem::new("Toggle STT (Super+M)", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append(&toggle_item)?;
        menu.append(&quit_item)?;

        // Create initial icon (inactive state)
        let icon = Self::create_icon(false)?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Voice Keyboard - Inactive")
            .with_icon(icon)
            .build()?;

        info!("System tray icon created");

        Ok(Self {
            _tray_icon: tray_icon,
            toggle_item: toggle_item,
            is_active,
        })
    }

    fn create_icon(active: bool) -> Result<Icon> {
        // Create a simple colored icon
        // 32x32 RGBA icon
        let size = 32;
        let mut rgba = vec![0u8; size * size * 4];

        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) * 4;
                
                // Create a circular icon
                let center_x = size as f32 / 2.0;
                let center_y = size as f32 / 2.0;
                let distance = ((x as f32 - center_x).powi(2) + (y as f32 - center_y).powi(2)).sqrt();
                let radius = size as f32 / 2.0 - 2.0;

                if distance <= radius {
                    if active {
                        // Green for active
                        rgba[idx] = 50;      // R
                        rgba[idx + 1] = 200; // G
                        rgba[idx + 2] = 50;  // B
                        rgba[idx + 3] = 255; // A
                    } else {
                        // Red for inactive
                        rgba[idx] = 200;     // R
                        rgba[idx + 1] = 50;  // G
                        rgba[idx + 2] = 50;  // B
                        rgba[idx + 3] = 255; // A
                    }
                } else {
                    // Transparent outside circle
                    rgba[idx + 3] = 0;
                }
            }
        }

        Icon::from_rgba(rgba, size as u32, size as u32)
            .map_err(|e| anyhow::anyhow!("Failed to create icon: {}", e))
    }

    pub fn update_icon(&mut self, active: bool) -> Result<()> {
        let icon = Self::create_icon(active)?;
        let tooltip = if active {
            "Voice Keyboard - Active"
        } else {
            "Voice Keyboard - Inactive"
        };

        self._tray_icon.set_icon(Some(icon))?;
        self._tray_icon.set_tooltip(Some(tooltip))?;
        
        debug!("Tray icon updated: {}", if active { "active" } else { "inactive" });
        Ok(())
    }

    pub fn handle_events(&mut self) -> Result<bool> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.toggle_item.id() {
                // Toggle state
                let mut active = self.is_active.lock();
                *active = !*active;
                let new_state = *active;
                drop(active);

                info!("Tray menu toggle: {}", if new_state { "active" } else { "inactive" });
                self.update_icon(new_state)?;
                return Ok(true); // State changed
            } else {
                // Quit item clicked
                info!("Quit requested from tray menu");
                std::process::exit(0);
            }
        }
        Ok(false) // No state change
    }
}

