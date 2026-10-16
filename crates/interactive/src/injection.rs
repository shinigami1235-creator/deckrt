//! The injection map's maths: the points, the totals per product and how a dose is written. The
//! points are show state only; nothing here reaches the deck.

use crate::object::InjectionMapConfig;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// In the object's own box, 0 to 1.
    pub x: f64,
    pub y: f64,
    /// Index into the configured products.
    pub product: usize,
    pub dose: f64,
}

/// `12`, `0.4`, `2.5`: up to two decimals, trailing zeros dropped, floating error rounded away.
pub fn format_dose(n: f64) -> String {
    if !n.is_finite() {
        return "0".into();
    }
    let r = (n * 100.0).round() / 100.0;
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// `12 U`, `0.4 mL`.
pub fn dose_text(n: f64, unit: &str) -> String {
    if unit.is_empty() { format_dose(n) } else { format!("{} {unit}", format_dose(n)) }
}

pub struct ProductTotal {
    pub product: usize,
    pub name: String,
    pub unit: String,
    pub total: f64,
    pub count: usize,
}

/// The running total per product, in product order, for the products with at least one point.
pub fn totals(config: &InjectionMapConfig, points: &[Point]) -> Vec<ProductTotal> {
    config
        .products
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let mine: Vec<&Point> = points.iter().filter(|pt| pt.product == i).collect();
            (!mine.is_empty()).then(|| ProductTotal {
                product: i,
                name: p.name.clone(),
                unit: p.unit.clone(),
                total: mine.iter().map(|pt| pt.dose).sum(),
                count: mine.len(),
            })
        })
        .collect()
}

/// `Toxin 12 U · Filler 0.4 mL`, or empty before the first point.
pub fn totals_line(config: &InjectionMapConfig, points: &[Point]) -> String {
    totals(config, points).iter().map(|t| format!("{} {}", t.name, dose_text(t.total, &t.unit))).collect::<Vec<_>>().join(" · ")
}

/// The dose a product's new points start with.
pub fn default_dose(config: &InjectionMapConfig, product: usize) -> f64 {
    config.products.get(product).map(|p| p.default_dose).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Product;

    #[test]
    fn doses_read_cleanly() {
        assert_eq!(format_dose(12.0), "12");
        assert_eq!(format_dose(0.1 + 0.2), "0.3");
        assert_eq!(format_dose(2.5), "2.5");
        assert_eq!(dose_text(4.0, "U"), "4 U");
        assert_eq!(format_dose(f64::NAN), "0");
    }

    #[test]
    fn totals_list_the_products_with_points() {
        let c = InjectionMapConfig {
            products: vec![
                Product { name: "Toxin".into(), unit: "U".into(), color: "#000000".into(), default_dose: 4.0 },
                Product { name: "Filler".into(), unit: "mL".into(), color: "#000000".into(), default_dose: 0.1 },
            ],
            show_total: true,
        };
        assert_eq!(totals_line(&c, &[]), "");
        let pts = [
            Point { x: 0.1, y: 0.1, product: 0, dose: 4.0 },
            Point { x: 0.2, y: 0.2, product: 0, dose: 8.0 },
            Point { x: 0.3, y: 0.3, product: 1, dose: 0.4 },
        ];
        assert_eq!(totals_line(&c, &pts), "Toxin 12 U · Filler 0.4 mL");
        assert_eq!(default_dose(&c, 1), 0.1);
        assert_eq!(default_dose(&c, 9), 0.0);
    }
}
