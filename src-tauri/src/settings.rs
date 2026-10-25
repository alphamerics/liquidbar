/// One placed control-center widget: its template id and top-left cell on
/// the panel's 4-column grid (pills span 2x1, panels 4x1, circles 1x1).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CcLayoutItem {
    pub id: String,
    pub x: i32,
    pub y: i32,
}

/// The control center's persisted visual toggles.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CcToggles {
    pub airplane: bool,
    pub night_light: bool,
}

/// Per-gadget micro settings, keyed by gadget id. Each gadget owns the shape
/// of its entry; the backend only persists the map opaquely (e.g.
/// `{"weather": {"unit": "celsius"}}`).
pub type GadgetSettings = std::collections::HashMap<String, serde_json::Value>;

/// The arrangement before the battery/exit widgets existed. A stored layout
/// matching it exactly means the user never customized the panel.
fn legacy_default_cc_layout() -> Vec<CcLayoutItem> {
    vec![
        CcLayoutItem { id: "wifi".to_string(), x: 0, y: 0 },
        CcLayoutItem { id: "bluetooth".to_string(), x: 2, y: 0 },
        CcLayoutItem { id: "focus".to_string(), x: 0, y: 1 },
        CcLayoutItem { id: "calendar".to_string(), x: 2, y: 1 },
        CcLayoutItem { id: "airplane".to_string(), x: 0, y: 2 },
        CcLayoutItem { id: "night-light".to_string(), x: 1, y: 2 },
        CcLayoutItem { id: "tray".to_string(), x: 2, y: 2 },
        CcLayoutItem { id: "settings".to_string(), x: 3, y: 2 },
        CcLayoutItem { id: "sound".to_string(), x: 0, y: 3 },
    ]
}

/// The layout format version: 0 means pre-versioning (the stored layout may
/// be the uncustomized pre-battery default), 1 means the layout was written
/// by the app after the battery/exit widgets existed (a migration has run or
/// the user edited the layout), so the legacy migration must never fire.
pub const CC_LAYOUT_VERSION_CURRENT: u32 = 1;

/// One-time migration: upgrades a stored layout that exactly matches the
/// pre-battery default to the current default (persisted by the caller), so
/// older installs see the new widgets. Only fires while the layout is still
/// at version 0: once any explicit layout write happens, `cc_layout_version`
/// is bumped (including by removals), so a user layout can never be treated
/// as uncustomized again.
pub fn upgrade_legacy_cc_layout(settings: &mut GlassbarSettings) -> bool {
    if settings.cc_layout_version == 0 && settings.cc_layout == legacy_default_cc_layout() {
        settings.cc_layout = default_cc_layout();
        settings.cc_layout_version = CC_LAYOUT_VERSION_CURRENT;
        true
    } else {
        false
    }
}

/// The control center's default widget arrangement.
pub fn default_cc_layout() -> Vec<CcLayoutItem> {
    vec![
        CcLayoutItem { id: "wifi".to_string(), x: 0, y: 0 },
        CcLayoutItem { id: "bluetooth".to_string(), x: 2, y: 0 },
        CcLayoutItem { id: "focus".to_string(), x: 0, y: 1 },
        CcLayoutItem { id: "calendar".to_string(), x: 2, y: 1 },
        CcLayoutItem { id: "airplane".to_string(), x: 0, y: 2 },
        CcLayoutItem { id: "night-light".to_string(), x: 1, y: 2 },
        CcLayoutItem { id: "tray".to_string(), x: 2, y: 2 },
        CcLayoutItem { id: "settings".to_string(), x: 3, y: 2 },
        CcLayoutItem { id: "sound".to_string(), x: 0, y: 3 },
        CcLayoutItem { id: "battery".to_string(), x: 0, y: 4 },
        CcLayoutItem { id: "exit".to_string(), x: 2, y: 4 },
    ]
}

/// All user-tunable values, edited in the settings window and applied live.
///
/// The glass material fields (refraction, blur, tint, outline, border,
/// turbulence, wobble) are `#[serde(skip_serializing)]`: they are NOT
/// persisted in this file — they live in the broker-owned global material
/// file (`%LOCALAPPDATA%\LiquidGlass\material.json`, the home every liquid-glass
/// app shares). They are populated from that file at load (see
/// `apply_material`) and written back to it on save (see `material_from`),
/// so the settings window edits the global material, never a private copy.
/// They still deserialize: `set_settings` round-trips the serialized state
/// (with the material keys re-injected) back through the struct, and skip
/// would silently zero those fields there.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct GlassbarSettings {
    pub reserve_space: bool,
    #[serde(skip_serializing)]
    pub refraction_strength: f64,
    #[serde(skip_serializing)]
    pub blur_radius: f64,
    /// Kawase blur chain level count (1..4): more levels blur deeper and
    /// smoother at the same tap cost.
    #[serde(skip_serializing)]
    pub blur_samples: f64,
    #[serde(skip_serializing)]
    pub tint_strength: f64,
    #[serde(skip_serializing)]
    pub tint_color: [f64; 3],
    #[serde(skip_serializing)]
    pub edge_band: f64,
    #[serde(skip_serializing)]
    pub chromatic_offset: f64,
    #[serde(skip_serializing)]
    pub chromatic_blend: f64,
    #[serde(skip_serializing)]
    pub outline_strength: f64,
    #[serde(skip_serializing)]
    pub outline_thickness: f64,
    #[serde(skip_serializing)]
    pub outline_angle: f64,
    #[serde(skip_serializing)]
    pub outline_mirror: f64,
    #[serde(skip_serializing)]
    pub outline_fade: f64,
    /// Uniform black border around every glass surface: thickness in
    /// physical pixels and constant opacity (0..1).
    #[serde(skip_serializing)]
    pub border_thickness: f64,
    #[serde(skip_serializing)]
    pub border_opacity: f64,
    #[serde(skip_serializing)]
    pub turbulence_strength: f64,
    #[serde(skip_serializing)]
    pub turbulence_scale: f64,
    #[serde(skip_serializing)]
    pub wobble: f64,
    pub pill_radius: f64,
    /// Taskbar resize animation: squash & stretch depth (0..1), spring
    /// bounciness (0..1) and one oscillation period in milliseconds.
    pub resize_squash: f64,
    pub resize_bounce: f64,
    pub resize_duration: f64,
    /// The invisible bottom-edge strip that reveals the auto-hidden taskbar:
    /// its regular height (the reveal trigger) and its taller height while
    /// the taskbar floats (both physical pixels, measured up from the
    /// screen's bottom edge; both bands span the whole monitor width).
    pub hover_strip_height: f64,
    pub hover_strip_hover_height: f64,
    /// Taskbar app icon size (CSS px), applied as a CSS variable by the
    /// webview.
    pub icon_size: f64,
    /// Gap between taskbar app icons (CSS px).
    pub icon_gap: f64,
    /// Ids of the enabled taskbar gadgets, in display order.
    pub enabled_gadgets: Vec<String>,
    /// Per-gadget micro settings, keyed by gadget id (opaque JSON entries).
    pub gadget_settings: GadgetSettings,
    /// Whether pinned apps render to the left of running apps in the bar.
    pub pinned_side_left: bool,
    /// Whether Glassbar hides Windows' own taskbar (Explorer's) entirely.
    pub hide_taskbar: bool,
    /// Stricter auto-hide: the taskbar fully hides whenever it is not
    /// hovered, even when no maximized window covers the strip (a reveal
    /// gesture brings it back). Not a default.
    pub hide_when_unhovered: bool,
    /// Whether the app launches itself when the user logs in (the Windows
    /// registry's per-user Run key). On by default; toggling the setting
    /// writes or deletes the registry entry live.
    pub launch_on_startup: bool,
    /// Whether the reveal gesture still works while a true fullscreen app
    /// is in the foreground. With this on, the bottom-edge strip only
    /// reveals the taskbar while the mouse cursor is VISIBLE — games hide
    /// the cursor during gameplay, so camera-driven cursor drift into the
    /// strip never reveals the bar. With this off, the strip is fully
    /// ignored under fullscreen apps.
    pub reveal_in_fullscreen: bool,
    /// The control center's widget layout, edited in the settings window's
    /// layout editor.
    pub cc_layout: Vec<CcLayoutItem>,
    /// The control center layout's format version. 0 means the layout was
    /// written before this field existed; the legacy migration only runs at
    /// version 0 and any explicit layout write bumps it (see
    /// `CC_LAYOUT_VERSION_CURRENT`).
    pub cc_layout_version: u32,
    /// The control center's persisted visual toggles.
    pub cc_toggles: CcToggles,
    /// Display order of the taskbar's pinned apps (process names), edited by
    /// dragging the buttons.
    pub taskbar_pin_order: Vec<String>,
    /// Display order of the taskbar's running (unpinned) apps, edited by
    /// dragging the buttons.
    pub taskbar_app_order: Vec<String>,
}

impl Default for GlassbarSettings {
    fn default() -> Self {
        let mut settings = Self {
            reserve_space: false,
            refraction_strength: 0.0,
            blur_radius: 0.0,
            blur_samples: 0.0,
            tint_strength: 0.0,
            tint_color: [0.0; 3],
            edge_band: 0.0,
            chromatic_offset: 0.0,
            chromatic_blend: 0.0,
            outline_strength: 0.0,
            outline_thickness: 0.0,
            outline_angle: 0.0,
            outline_mirror: 0.0,
            outline_fade: 0.0,
            border_thickness: 0.0,
            border_opacity: 0.0,
            turbulence_strength: 0.0,
            turbulence_scale: 0.0,
            wobble: 0.0,
            pill_radius: 34.0,
            resize_squash: 0.3,
            resize_bounce: 0.3,
            resize_duration: 600.0,
            hover_strip_height: 8.0,
            hover_strip_hover_height: 100.0,
            icon_size: 45.0,
            icon_gap: 3.0,
            enabled_gadgets: vec!["weather".to_string(), "clock".to_string()],
            gadget_settings: GadgetSettings::new(),
            pinned_side_left: true,
            hide_taskbar: true,
            launch_on_startup: true,
            hide_when_unhovered: false,
            reveal_in_fullscreen: true,
            cc_layout: default_cc_layout(),
            cc_layout_version: CC_LAYOUT_VERSION_CURRENT,
            cc_toggles: CcToggles::default(),
            taskbar_pin_order: Vec::new(),
            taskbar_app_order: Vec::new(),
        };
        // The material fields come from the shared material's defaults, not
        // taskbar-local literals: the global material file is the material's
        // single home, and a reset must reset THAT.
        apply_material(&mut settings, &liquid_glass::Material::default());
        settings
    }
}

/// Overwrites the settings' material fields with the shared material's.
/// Called at load (the global file wins over any stale local keys) and by
/// `Default` (the material's defaults are the defaults).
pub fn apply_material(settings: &mut GlassbarSettings, material: &liquid_glass::Material) {
    settings.refraction_strength = material.refraction_strength as f64;
    settings.blur_radius = material.blur_radius as f64;
    settings.blur_samples = material.blur_samples as f64;
    settings.tint_strength = material.tint_strength as f64;
    settings.tint_color = [
        material.glass_tint[0] as f64,
        material.glass_tint[1] as f64,
        material.glass_tint[2] as f64,
    ];
    settings.edge_band = material.edge_band as f64;
    settings.chromatic_offset = material.chromatic_offset as f64;
    settings.chromatic_blend = material.chromatic_blend as f64;
    settings.outline_strength = material.outline_strength as f64;
    settings.outline_thickness = material.outline_thickness as f64;
    settings.outline_angle = material.outline_angle as f64;
    settings.outline_mirror = material.outline_mirror as f64;
    settings.outline_fade = material.outline_fade as f64;
    settings.border_thickness = material.border_thickness as f64;
    settings.border_opacity = material.border_opacity as f64;
    settings.turbulence_strength = material.turbulence_strength as f64;
    settings.turbulence_scale = material.turbulence_scale as f64;
    settings.wobble = material.reveal_wobble as f64;
}

/// Serializes the settings into the JSON the frontend expects: the plain
/// serde output omits the `#[serde(skip)]` material fields (their home is
/// the global material file), but every window reads them from the settings
/// snapshot — so the material's values are merged back in here, under the
/// same camelCase keys the shared material file uses.
pub fn full_json(settings: &GlassbarSettings) -> serde_json::Value {
    let mut value = serde_json::to_value(settings).expect("settings serialize");
    let material = serde_json::to_value(material_from(settings)).expect("material serialize");
    if let (Some(fields), Some(material_fields)) = (value.as_object_mut(), material.as_object()) {
        for (key, material_field) in material_fields {
            fields.insert(key.clone(), material_field.clone());
        }
    }
    value
}

/// Extracts the shared material from the settings' material fields (the
/// inverse of [`apply_material`]); used by the save path to write the
/// global material file.
pub fn material_from(settings: &GlassbarSettings) -> liquid_glass::Material {
    liquid_glass::Material {
        refraction_strength: settings.refraction_strength as f32,
        blur_radius: settings.blur_radius as f32,
        blur_samples: settings.blur_samples as f32,
        tint_strength: settings.tint_strength as f32,
        glass_tint: [
            settings.tint_color[0] as f32,
            settings.tint_color[1] as f32,
            settings.tint_color[2] as f32,
        ],
        edge_band: settings.edge_band as f32,
        chromatic_offset: settings.chromatic_offset as f32,
        chromatic_blend: settings.chromatic_blend as f32,
        outline_strength: settings.outline_strength as f32,
        outline_thickness: settings.outline_thickness as f32,
        outline_angle: settings.outline_angle as f32,
        outline_mirror: settings.outline_mirror as f32,
        outline_fade: settings.outline_fade as f32,
        border_thickness: settings.border_thickness as f32,
        border_opacity: settings.border_opacity as f32,
        turbulence_strength: settings.turbulence_strength as f32,
        turbulence_scale: settings.turbulence_scale as f32,
        reveal_wobble: settings.wobble as f32,
    }
}

/// Maps the user settings onto the shader params — shared by set_settings
/// and the startup path, so saved settings apply without a manual toggle.
pub fn shader_params_from(settings: &GlassbarSettings) -> liquid_glass::GlassShaderParams {
    liquid_glass::GlassShaderParams {
        refraction_strength: settings.refraction_strength as f32,
        blur_radius_texels: settings.blur_radius as f32,
        blur_samples: settings.blur_samples as f32,
        tint_strength: settings.tint_strength as f32,
        glass_tint: [
            settings.tint_color[0] as f32,
            settings.tint_color[1] as f32,
            settings.tint_color[2] as f32,
        ],
        edge_band: settings.edge_band as f32,
        chromatic_offset_texels: settings.chromatic_offset as f32,
        chromatic_blend: settings.chromatic_blend as f32,
        outline_strength: settings.outline_strength as f32,
        outline_thickness_px: settings.outline_thickness as f32,
        outline_angle_degrees: settings.outline_angle as f32,
        outline_mirror: settings.outline_mirror as f32,
        outline_fade_px: settings.outline_fade as f32,
        border_thickness_px: settings.border_thickness as f32,
        border_opacity: settings.border_opacity as f32,
        turbulence_strength: settings.turbulence_strength as f32,
        turbulence_scale: settings.turbulence_scale as f32,
        reveal_wobble: settings.wobble as f32,
        capsule_radius: (settings.pill_radius as f32 / 68.0).clamp(0.0, 0.5),
        ..Default::default()
    }
}
