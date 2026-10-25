use glassbar_lib::GlassbarSettings;

#[test]
fn parse_real_settings_file() {
    let raw = std::fs::read_to_string(concat!(
        env!("USERPROFILE"),
        r"\AppData\Roaming\org.newmoonworks.glassbar\settings.json"
    ))
    .expect("settings.json readable");
    let settings: GlassbarSettings = serde_json::from_str(&raw).expect("parses");
    println!("cc_layout = {:?}", settings.cc_layout);
    println!("cc_toggles = {:?}", settings.cc_toggles);
    println!("gadget_settings = {:?}", settings.gadget_settings);
    println!("hide_when_unhovered = {}", settings.hide_when_unhovered);
}
