use eframe::egui;
use framework_lib::chromium_ec::{CrosEc, CrosEcDriver, EcError};
use framework_lib::chromium_ec::commands::RgbS;

const COLOR_COUNT: usize = 8;
const PRESET_SPECTRUM: [u32; COLOR_COUNT] = [
    0xFF0000, 0xFF7F00, 0xFFFF00, 0x00FF00, 0x0000FF, 0x4B0082, 0x9400D3, 0xFFFFFF,
];
const PRESET_MATRIX: [u32; COLOR_COUNT] = [
    0x00FF66, 0x00CC44, 0x009933, 0x006622, 0x00FF99, 0x00CC66, 0x009944, 0x006633,
];
const PRESET_AZURE: [u32; COLOR_COUNT] = [
    0x0C0CFF, 0x1A1AFF, 0x2B2BFF, 0x3C3CFF, 0x1A4FFF, 0x2E5FFF, 0x4370FF, 0x5880FF,
];
const PRESET_NEON_CITY: [u32; COLOR_COUNT] = [
    0xFF00FF, 0x00FFFF, 0x9400D3, 0xFF0099, 0x00CCFF, 0x8A2BE2, 0xFF1493, 0x00BFFF,
];
const PRESET_SOLAR_FLARE: [u32; COLOR_COUNT] = [
    0xFF4500, 0xFF8C00, 0xFFA500, 0xFFD700, 0xFF6347, 0xFF7F50, 0xFFD700, 0xFFFF00,
];
const PRESET_ABYSS: [u32; COLOR_COUNT] = [
    0x000080, 0x00008B, 0x191970, 0x0000CD, 0x4169E1, 0x0000FF, 0x1E90FF, 0x00BFFF,
];
const PRESET_CANOPY: [u32; COLOR_COUNT] = [
    0x006400, 0x228B22, 0x32CD32, 0x90EE90, 0x008000, 0x6B8E23, 0x556B2F, 0x8FBC8F,
];

/// Convert a raw 24-bit RGB value into the EC payload struct.
fn rgb_from_u32(value: u32) -> RgbS {
    RgbS {
        r: ((value & 0x00FF_0000) >> 16) as u8,
        g: ((value & 0x0000_FF00) >> 8) as u8,
        b: (value & 0x0000_00FF) as u8,
    }
}

/// Apply RGB colors starting at a given key index using the Framework EC.
fn apply_colors(start_key: u8, colors: Vec<RgbS>) -> Result<(), EcError> {
    let ec = CrosEc::new();
    ec.rgbkbd_set_color(start_key, colors)
}

/// Set the main fan's duty cycle (0-100%) using the Framework EC's `PwmSetFanDuty` command.
///
/// The standard Chromium EC payload for EC_CMD_PWM_SET_FAN_DUTY (0x0024) is a single
/// packed struct: `struct ec_params_pwm_set_fan_duty { uint32_t percent; } __packed;`.
/// Note this differs from the generic PWM_SET_DUTY command — fan duty takes only the
/// percentage, with no leading index byte. This command returns no response body.
fn set_fan_duty(duty_percent: u8) -> Result<(), EcError> {
    let ec = CrosEc::new();

    // Clamp into the valid 0-100% range before sending to the EC.
    let duty = duty_percent.min(100);

    // `percent` is a 32-bit little-endian value, not a single byte — send all four
    // bytes so the EC reads the payload correctly (same as framework-tool / ectool).
    let request = (duty as u32).to_le_bytes();

    // `send_command` comes from the `CrosEcDriver` trait (now in scope). This command
    // has no response body, so we use the low-level path and ignore any returned bytes.
    ec.send_command(0x0024u16, 0, &request)?;
    Ok(())
}

/// Convert a `RgbS` color to a hex string (`#RRGGBB`).
fn rgb_to_hex_string(color: RgbS) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// Provide a user-friendly explanation for an EC error, including privilege guidance.
fn format_ec_error(err: &EcError) -> String {
    match err {
        EcError::DeviceError(message) if message.contains("Not a Framework Laptop") => {
            "EC access denied: SMBIOS check failed. Run this tool with administrative \
privileges (sudo) on a Framework system so the EC can be reached."
                .to_string()
        }
        EcError::DeviceError(message) => format!("EC device error: {message}"),
        EcError::Response(status) => {
            format!("EC responded with status {:?}", status)
        }
        EcError::UnknownResponseCode(code) => {
            format!("EC returned unknown response code 0x{code:X}.")
        }
    }
}

fn color32_from_rgb(color: RgbS) -> egui::Color32 {
    egui::Color32::from_rgb(color.r, color.g, color.b)
}

fn rgb_from_color32(color: egui::Color32) -> RgbS {
    RgbS {
        r: color.r(),
        g: color.g(),
        b: color.b(),
    }
}

#[derive(Clone, Copy)]
enum StatusKind {
    Info,
    Success,
    Error,
}

struct StatusMessage {
    kind: StatusKind,
    text: String,
}

struct FanRgbApp {
    start_key: u8,
    fan_duty: u8,
    colors: [egui::Color32; COLOR_COUNT],
    status: Option<StatusMessage>,
    lights_enabled: bool,
    /// When checked, "Write to controller" updates only the LEDs and leaves the
    /// fan controller untouched.
    led_only: bool,
}

impl FanRgbApp {
    fn new() -> Self {
        let colors = PRESET_SPECTRUM
            .iter()
            .map(|color| color32_from_rgb(rgb_from_u32(*color)))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or([egui::Color32::BLACK; COLOR_COUNT]);

        Self {
            start_key: 0,
            // 0 means "no fan choice made yet" — the fan is only written once the
            // user explicitly picks 50% or 100%, so we never surprise them.
            fan_duty: 0,
            colors,
            status: Some(StatusMessage {
                kind: StatusKind::Info,
                text: "Pick a fan duty (50% / 100%), adjust the colors, then press 'Write to controller'.".to_string(),
            }),
            lights_enabled: true,
            led_only: false,
        }
    }

    fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            kind,
            text: text.into(),
        });
    }

    fn current_colors(&self) -> Vec<RgbS> {
        self.colors.iter().copied().map(rgb_from_color32).collect()
    }

    fn apply_palette(&mut self, palette: &[u32]) {
        for idx in 0..COLOR_COUNT {
            let value = palette[idx % palette.len()];
            self.colors[idx] = color32_from_rgb(rgb_from_u32(value));
        }
        self.lights_enabled = true;
    }

    /// Write the current colors (and, unless `led_only` is set, the fan duty) to the EC.
    fn apply(&mut self) {
        // Lighting step: either turn LEDs off or write the chosen colors.
        if !self.lights_enabled {
            match self.turn_off_lights() {
                Ok(message) => self.set_status(StatusKind::Info, message),
                Err(err) => self.set_status(StatusKind::Error, err),
            }
            return;
        }

        // Fan step: only override the fan if one was actually chosen AND the user
        // hasn't asked for LED-only writes.
        if !self.led_only {
            match set_fan_duty(self.fan_duty) {
                Ok(()) => {}
                Err(err) => {
                    self.set_status(StatusKind::Error, format_ec_error(&err));
                    return;
                }
            }
        }

        // Colors step: report success with a combined message.
        match apply_colors(self.start_key, self.current_colors()) {
            Ok(()) => self.set_status(
                StatusKind::Success,
                if self.led_only {
                    format!("Wrote {} colors at key {}", COLOR_COUNT, self.start_key)
                } else {
                    format!(
                        "Wrote {} colors at key {}, main fan at {}%",
                        COLOR_COUNT, self.start_key, self.fan_duty
                    )
                },
            ),
            Err(err) => self.set_status(StatusKind::Error, format_ec_error(&err)),
        }
    }

    fn turn_off_lights(&mut self) -> Result<String, String> {
        let off = vec![RgbS { r: 0, g: 0, b: 0 }; COLOR_COUNT];
        apply_colors(self.start_key, off)
            .map(|_| "Fan lighting disabled".to_string())
            .map_err(|err| format_ec_error(&err))
    }

    fn apply_twilight(&mut self) {
        let first = self.colors.first().copied().unwrap_or(egui::Color32::BLACK);
        let last = self.colors.last().copied().unwrap_or(egui::Color32::BLACK);

        for (idx, color) in self.colors.iter_mut().enumerate() {
            let t = idx as f32 / (COLOR_COUNT.saturating_sub(1) as f32).max(1.0);
            *color = egui::Color32::from_rgb(
                Self::lerp_channel(first.r(), last.r(), t),
                Self::lerp_channel(first.g(), last.g(), t),
                Self::lerp_channel(first.b(), last.b(), t),
            );
        }

        self.lights_enabled = true;
    }

    fn lerp_channel(a: u8, b: u8, t: f32) -> u8 {
        let a = a as f32;
        let b = b as f32;
        (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
    }
}

impl eframe::App for FanRgbApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add(
                egui::Label::new(
                    "Note: This program requires sudo on a Framework Linux system.",
                )
                .wrap(true),
            );
        });

        egui::SidePanel::right("presets_panel")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Presets");

                if ui.button("Twilight").clicked() {
                    self.apply_twilight();
                }
                if ui.button("Matrix").clicked() {
                    self.apply_palette(&PRESET_MATRIX);
                }
                if ui.button("Azure").clicked() {
                    self.apply_palette(&PRESET_AZURE);
                }
                if ui.button("Neon City").clicked() {
                    self.apply_palette(&PRESET_NEON_CITY);
                }
                if ui.button("Solar Flare").clicked() {
                    self.apply_palette(&PRESET_SOLAR_FLARE);
                }
                if ui.button("Abyss").clicked() {
                    self.apply_palette(&PRESET_ABYSS);
                }
                if ui.button("Canopy").clicked() {
                    self.apply_palette(&PRESET_CANOPY);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Start key");
                ui.add(
                    egui::Slider::new(&mut self.start_key, 0..=255)
                        .text("index")
                        .clamp_to_range(true),
                );
            });

            ui.separator();
            ui.heading("Colors");

            for idx in 0..COLOR_COUNT {
                let mut color_value = self.colors[idx];
                let mut updated = false;

                ui.horizontal(|ui| {
                    ui.label(format!("Zone {}", idx + 1));
                    let mut egui_color = color_value;
                    let response = egui::color_picker::color_edit_button_srgba(
                        ui,
                        &mut egui_color,
                        egui::color_picker::Alpha::Opaque,
                    );

                    if response.changed() {
                        color_value = egui_color;
                        updated = true;
                    }

                    ui.label(rgb_to_hex_string(rgb_from_color32(color_value)));
                });

                if updated {
                    self.colors[idx] = color_value;
                    self.lights_enabled = true;
                } else {
                    self.colors[idx] = color_value;
                }
            }

            ui.separator();
            ui.heading("Fan override");
            ui.horizontal(|ui| {
                if ui.button("50%").clicked() {
                    self.fan_duty = 50;
                }
                if ui.button("100%").clicked() {
                    self.fan_duty = 100;
                }
            });

            ui.checkbox(&mut self.led_only, "LEDs only (skip fan override)");

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Write to controller").clicked() {
                    self.apply();
                }

                let toggle_label = if self.lights_enabled {
                    "Turn off lighting"
                } else {
                    "Turn on lighting"
                };
                if ui.button(toggle_label).clicked() {
                    self.lights_enabled = !self.lights_enabled;
                }
            });

            if let Some(status) = &self.status {
                ui.separator();
                let color = match status.kind {
                    StatusKind::Info => egui::Color32::LIGHT_GRAY,
                    StatusKind::Success => egui::Color32::from_rgb(0, 200, 83),
                    StatusKind::Error => egui::Color32::from_rgb(209, 71, 78),
                };
                ui.colored_label(color, &status.text);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 480.0])
            .with_min_inner_size([520.0, 360.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Cortinarius - A Framework Desktop fan controller",
        native_options,
        Box::new(|_cc| Box::new(FanRgbApp::new())),
    )
}
