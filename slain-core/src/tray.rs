//! System Tray & Security Camera PIP Module
//!
//! Features:
//! - System tray icon with quick access menu
//! - Start on Windows startup
//! - Picture-in-Picture security camera feeds
//! - Multiple camera support (RTSP, HTTP, local)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Stub types for system tray (replaces Tauri dependency)
// These allow the module to compile without Tauri
// ============================================================================

/// Stub for Tauri AppHandle
pub struct AppHandle;

impl AppHandle {
    /// Stub - returns None since we're not using Tauri
    pub fn get_webview_window(&self, _label: &str) -> Option<WebviewWindow> {
        None
    }
}

/// Stub for webview window
pub struct WebviewWindow;

impl WebviewWindow {
    pub fn show(&self) -> Result<(), ()> { Ok(()) }
    pub fn set_focus(&self) -> Result<(), ()> { Ok(()) }
}

/// Stub for system tray menu
pub struct SystemTrayMenu {
    items: Vec<String>,
}

impl SystemTrayMenu {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    
    pub fn add_item(mut self, item: CustomMenuItem) -> Self {
        self.items.push(item.id);
        self
    }
    
    pub fn add_native_item(self, _item: SystemTrayMenuItem) -> Self {
        self
    }
    
    pub fn add_submenu(mut self, submenu: SystemTraySubmenu) -> Self {
        self.items.push(submenu.title);
        self
    }
}

/// Stub for custom menu item
pub struct CustomMenuItem {
    id: String,
    #[allow(dead_code)]
    title: String,
}

impl CustomMenuItem {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self { id: id.into(), title: title.into() }
    }
}

/// Stub for native menu items
pub enum SystemTrayMenuItem {
    Separator,
}

/// Stub for submenu
pub struct SystemTraySubmenu {
    title: String,
    #[allow(dead_code)]
    menu: SystemTrayMenu,
}

impl SystemTraySubmenu {
    pub fn new(title: impl Into<String>, menu: SystemTrayMenu) -> Self {
        Self { title: title.into(), menu }
    }
}

/// Stub for tray events
pub enum SystemTrayEvent {
    LeftClick { position: (f64, f64), size: (f64, f64) },
    MenuItemClick { id: String },
}

// ============================================================================
// Security Camera Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityCamera {
    pub id: String,
    pub name: String,
    pub url: String,           // RTSP, HTTP, or file path
    pub camera_type: CameraType,
    pub enabled: bool,
    pub pip_position: PipPosition,
    pub pip_size: PipSize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum CameraType {
    Rtsp,           // rtsp://user:pass@192.168.1.x:554/stream
    Http,           // http://192.168.1.x/video.mjpg
    HttpMjpeg,      // Motion JPEG stream
    Onvif,          // ONVIF compatible
    File,           // Local file or device
    Usb,            // USB webcam
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PipPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Custom { x: i32, y: i32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PipSize {
    Small,      // 160x120
    Medium,     // 320x240
    Large,      // 480x360
    Custom { width: u32, height: u32 },
}

impl PipSize {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            PipSize::Small => (160, 120),
            PipSize::Medium => (320, 240),
            PipSize::Large => (480, 360),
            PipSize::Custom { width, height } => (*width, *height),
        }
    }
}

// ============================================================================
// App Settings
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub start_on_boot: bool,
    pub start_minimized: bool,
    pub show_tray_icon: bool,
    pub minimize_to_tray: bool,
    pub cameras: Vec<SecurityCamera>,
    pub pip_opacity: f32,       // 0.0 - 1.0
    pub pip_always_on_top: bool,
    pub pip_click_through: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_on_boot: false,
            start_minimized: false,
            show_tray_icon: true,
            minimize_to_tray: true,
            cameras: Vec::new(),
            pip_opacity: 1.0,
            pip_always_on_top: true,
            pip_click_through: false,
        }
    }
}

// ============================================================================
// Windows Startup Registration
// ============================================================================

#[cfg(target_os = "windows")]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    let key = hkcu.open_subkey_with_flags(path, KEY_SET_VALUE)
        .map_err(|e| format!("Failed to open registry: {}", e))?;
    
    let app_name = "SLAIN Video Player";
    
    if enable {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?;
        
        // Add --minimized flag for startup
        let startup_cmd = format!("\"{}\" --minimized", exe_path.display());
        
        key.set_value(app_name, &startup_cmd)
            .map_err(|e| format!("Failed to set registry value: {}", e))?;
    } else {
        // Ignore error if key doesn't exist
        let _ = key.delete_value(app_name);
    }
    
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    // Linux: Create .desktop file in ~/.config/autostart/
    // macOS: Use launchd plist
    Err("Autostart not implemented for this platform".to_string())
}

pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
        
        if let Ok(key) = hkcu.open_subkey(path) {
            let result: Result<String, _> = key.get_value("SLAIN Video Player");
            return result.is_ok();
        }
        false
    }
    
    #[cfg(not(target_os = "windows"))]
    false
}

// ============================================================================
// System Tray
// ============================================================================

pub fn create_tray_menu(cameras: &[SecurityCamera]) -> SystemTrayMenu {
    let mut menu = SystemTrayMenu::new();
    
    // Main controls
    menu = menu
        .add_item(CustomMenuItem::new("open", "Open SLAIN"))
        .add_native_item(SystemTrayMenuItem::Separator);
    
    // Camera submenu if cameras are configured
    if !cameras.is_empty() {
        let mut camera_menu = SystemTrayMenu::new();
        
        for cam in cameras {
            let label = if cam.enabled {
                format!("✓ {}", cam.name)
            } else {
                cam.name.clone()
            };
            camera_menu = camera_menu.add_item(
                CustomMenuItem::new(format!("cam_{}", cam.id), label)
            );
        }
        
        camera_menu = camera_menu
            .add_native_item(SystemTrayMenuItem::Separator)
            .add_item(CustomMenuItem::new("cam_all_on", "Show All Cameras"))
            .add_item(CustomMenuItem::new("cam_all_off", "Hide All Cameras"))
            .add_item(CustomMenuItem::new("cam_settings", "Camera Settings..."));
        
        menu = menu
            .add_submenu(SystemTraySubmenu::new("Security Cameras", camera_menu))
            .add_native_item(SystemTrayMenuItem::Separator);
    } else {
        menu = menu
            .add_item(CustomMenuItem::new("cam_add", "Add Security Camera..."))
            .add_native_item(SystemTrayMenuItem::Separator);
    }
    
    // PIP controls
    let pip_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("pip_small", "Small (160x120)"))
        .add_item(CustomMenuItem::new("pip_medium", "Medium (320x240)"))
        .add_item(CustomMenuItem::new("pip_large", "Large (480x360)"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new("pip_top_left", "↖ Top Left"))
        .add_item(CustomMenuItem::new("pip_top_right", "↗ Top Right"))
        .add_item(CustomMenuItem::new("pip_bottom_left", "↙ Bottom Left"))
        .add_item(CustomMenuItem::new("pip_bottom_right", "↘ Bottom Right"));
    
    menu = menu.add_submenu(SystemTraySubmenu::new("PIP Settings", pip_menu));
    
    // Settings
    let settings_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("autostart", 
            if is_autostart_enabled() { "✓ Start with Windows" } else { "Start with Windows" }
        ))
        .add_item(CustomMenuItem::new("minimize_tray", "Minimize to Tray"));
    
    menu = menu
        .add_submenu(SystemTraySubmenu::new("Settings", settings_menu))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new("quit", "Quit SLAIN"));
    
    menu
}

pub fn handle_tray_event(app: &AppHandle, event: SystemTrayEvent) {
    match event {
        SystemTrayEvent::LeftClick { .. } => {
            // Show/focus main window
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        SystemTrayEvent::MenuItemClick { id, .. } => {
            handle_menu_click(app, &id);
        }
        _ => {}
    }
}

fn handle_menu_click(app: &AppHandle, id: &str) {
    match id {
        "open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "quit" => {
            app.exit(0);
        }
        "autostart" => {
            let currently_enabled = is_autostart_enabled();
            if let Err(e) = set_autostart(!currently_enabled) {
                eprintln!("Failed to toggle autostart: {}", e);
            }
            // Update menu
            update_tray_menu(app);
        }
        id if id.starts_with("cam_") => {
            handle_camera_action(app, id);
        }
        id if id.starts_with("pip_") => {
            handle_pip_action(app, id);
        }
        _ => {}
    }
}

fn handle_camera_action(app: &AppHandle, action: &str) {
    // Send to frontend
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("camera-action", action);
    }
}

fn handle_pip_action(app: &AppHandle, action: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("pip-action", action);
    }
}

fn update_tray_menu(app: &AppHandle) {
    // In real implementation, load cameras from settings
    let cameras: Vec<SecurityCamera> = Vec::new();
    let menu = create_tray_menu(&cameras);
    
    if let Some(tray) = app.tray_handle_by_id("main") {
        let _ = tray.set_menu(menu);
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================


pub fn get_app_settings() -> AppSettings {
    // Load from file or return defaults
    load_settings().unwrap_or_default()
}


pub fn save_app_settings(settings: AppSettings) -> Result<(), String> {
    save_settings(&settings)?;
    
    // Apply autostart setting
    set_autostart(settings.start_on_boot)?;
    
    Ok(())
}


pub fn toggle_autostart() -> Result<bool, String> {
    let currently_enabled = is_autostart_enabled();
    set_autostart(!currently_enabled)?;
    Ok(!currently_enabled)
}


pub fn get_autostart_status() -> bool {
    is_autostart_enabled()
}


pub fn add_security_camera(camera: SecurityCamera) -> Result<(), String> {
    let mut settings = load_settings().unwrap_or_default();
    settings.cameras.push(camera);
    save_settings(&settings)
}


pub fn remove_security_camera(camera_id: String) -> Result<(), String> {
    let mut settings = load_settings().unwrap_or_default();
    settings.cameras.retain(|c| c.id != camera_id);
    save_settings(&settings)
}


pub fn toggle_camera_pip(camera_id: String) -> Result<bool, String> {
    let mut settings = load_settings().unwrap_or_default();
    
    for cam in &mut settings.cameras {
        if cam.id == camera_id {
            cam.enabled = !cam.enabled;
            save_settings(&settings)?;
            return Ok(cam.enabled);
        }
    }
    
    Err("Camera not found".to_string())
}


pub fn get_cameras() -> Vec<SecurityCamera> {
    load_settings().map(|s| s.cameras).unwrap_or_default()
}

// ============================================================================
// Settings Persistence
// ============================================================================

fn settings_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("SLAIN");
    std::fs::create_dir_all(&path).ok();
    path.push("settings.json");
    path
}

fn load_settings() -> Result<AppSettings, String> {
    let path = settings_path();
    
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    
    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pip_size() {
        assert_eq!(PipSize::Small.dimensions(), (160, 120));
        assert_eq!(PipSize::Medium.dimensions(), (320, 240));
    }
    
    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert!(!settings.start_on_boot);
        assert!(settings.show_tray_icon);
    }
}
