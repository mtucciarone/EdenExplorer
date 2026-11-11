use super::AppIcon;

#[test]
fn every_app_icon_has_a_lucide_asset() {
    for icon in AppIcon::ALL {
        let svg = icon.lucide_svg();
        assert!(!svg.is_empty(), "missing Lucide asset for {icon:?}");
        assert!(svg.trim_start().starts_with("<svg"));
    }
}
