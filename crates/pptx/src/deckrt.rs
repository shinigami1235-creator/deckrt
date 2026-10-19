//! deckrt: picture saturation in the file. PowerPoint keeps it as an `a14:saturation` effect on
//! an `a14:imgLayer`; upstream kept saturation in the model only, so it was lost on save.
//!
//! deckrt writes the layer over the picture the blip shows (the same relationship), since the
//! saturation is applied when the slide is drawn. A picture whose layer is a different image was
//! saved by PowerPoint with the effect already in the blip, so its saturation isn't read again.

use deckcraft_model::style::PictureAdjust;

use crate::read::Part;
use crate::write::dml::pct;
use crate::xml::{A, El, W};

/// The saturation worth writing: set and not 100%.
pub fn saturation_of(adj: &PictureAdjust) -> Option<f64> {
    adj.saturation.filter(|v| v.is_finite() && (v - 1.0).abs() > 1e-6).map(|v| v.clamp(0.0, 4.0))
}

/// The `a:extLst` inside `a:blip` holding the saturation, over the picture `rid` names.
pub fn write_saturation(w: &mut W, rid: &str, sat: f64) {
    w.open0("a:extLst");
    w.open("a:ext", A::new().a("uri", "{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}"));
    w.open("a14:imgProps", A::new().a("xmlns:a14", crate::opc::NS_A14));
    w.open("a14:imgLayer", A::new().a("r:embed", rid));
    w.open0("a14:imgEffect");
    w.empty("a14:saturation", A::new().a("sat", pct(sat)));
    w.close("a14:imgEffect");
    w.close("a14:imgLayer");
    w.close("a14:imgProps");
    w.close("a:ext");
    w.close("a:extLst");
}

/// The saturation on `blip`'s image layer when the layer is the blip's own picture.
pub fn read_saturation(part: &Part, blip: &El) -> Option<f64> {
    let layer = blip.child("extLst")?.find("imgLayer")?;
    let target = |e: &El| e.attr("r:embed").and_then(|id| part.rels.get(id)).map(|r| r.target.clone());
    if target(layer)? != target(blip)? {
        return None;
    }
    let sat = layer.elements().filter(|e| e.local() == "imgEffect").flat_map(|e| e.elements()).find(|e| e.local() == "saturation")?;
    let v = sat.attr("sat").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(100_000.0) / 100_000.0;
    v.is_finite().then(|| v.clamp(0.0, 4.0))
}
