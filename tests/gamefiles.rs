//! End-to-end test of reading weapon/ammo data from game archives.
//! Needs the fixture from tools/make_archive_fixture.py; set
//! COTW_ARCHIVE_FIXTURE=<out-dir> (skipped otherwise).

use cotwmod::gamedata::{fit_engine_to_tables, Engine, GameData};
use cotwmod::gamefiles::{archives_dir, build, Progress};
use std::path::PathBuf;

#[test]
fn discovery_matches_reference_database() {
    let Ok(dir) = std::env::var("COTW_ARCHIVE_FIXTURE") else {
        eprintln!("COTW_ARCHIVE_FIXTURE not set - skipped");
        return;
    };
    let ad = archives_dir(&PathBuf::from(dir)).expect("archives_win64");
    let t = std::time::Instant::now();
    let g = build(&ad, &Progress::default()).expect("build");
    eprintln!("built in {:?}: {} weapons, {} ammo, {} tunings", t.elapsed(), g.weapons.len(), g.ammo.len(), g.tunings.len());
    let r = GameData::embedded();

    let missing: Vec<_> = r.ammo.keys().filter(|k| !g.ammo.contains_key(*k)).collect();
    eprintln!("missing ammo: {missing:?}");
    assert_eq!(g.weapons.keys().collect::<Vec<_>>(), r.weapons.keys().collect::<Vec<_>>(), "weapon set");
    for (k, w) in &r.weapons {
        let x = &g.weapons[k];
        assert_eq!(x.display, w.display, "{k}");
        assert_eq!(x.tuning, w.tuning, "{k}");
        assert_eq!(x.ammo, w.ammo, "{k}");
        assert_eq!(x.hash, w.hash, "{k}");
        let (a, b) = (&g.tunings[&x.tuning], &r.tunings[&w.tuning]);
        assert_eq!(a.zeros.len(), b.zeros.len());
        assert!((a.ke - b.ke).abs() < 1e-3 && (a.gravity - b.gravity).abs() < 1e-4);
        for (za, zb) in a.zeros.iter().zip(&b.zeros) {
            assert!((za.distance - zb.distance).abs() < 1e-4 && (za.angle - zb.angle).abs() < 1e-5, "{k}");
            assert_eq!(za.overrides.len(), zb.overrides.len(), "{k} overrides");
            for (ak, v) in &zb.overrides {
                assert!((za.overrides[ak] - v).abs() < 1e-5);
            }
        }
        for ak in &w.ammo {
            let (a, b) = (&g.ammo[ak], &r.ammo[ak]);
            assert_eq!(a.display, b.display);
            for (p, q) in [(a.mass, b.mass), (a.diameter, b.diameter), (a.cd, b.cd), (a.ke, b.ke), (a.max_range, b.max_range)] {
                assert!((p - q).abs() < 1e-6, "{ak}");
            }
        }
    }
    // the automatic calibration works on freshly read data
    let (e, rb, mg, n) = fit_engine_to_tables(&g, &Engine::default()).unwrap();
    eprintln!("auto-fit {e:?} bows {rb:.2} mrad rms, guns {mg:.2} mrad median, {n} samples");
    assert!(rb < 1.6 && mg < 0.6);
}
