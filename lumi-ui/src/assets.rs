use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(pub ExtraIcons, [Scale, ScrollText, TrendingUp, Wallet]);

/// The icons the lumi views use on native platforms: GPUI Component's default
/// set plus the catalog icons in [`ExtraIcons`]. Register it with
/// `Application::with_assets`. A web shell composes [`ExtraIcons`] with the
/// endpoint-based `Assets` of its deployment instead.
#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Copy, Debug, Default)]
pub struct LumiAssets;

#[cfg(not(target_family = "wasm"))]
impl AssetSource for LumiAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        // The extra icons go first: the component set reports an unknown
        // path as an error rather than `None`.
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
