use cotwmod::data::Trophy;
use cotwmod::population::PopFile;
use rand::SeedableRng;
use std::path::PathBuf;

fn fixture() -> PopFile {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/animal_population_0");
    PopFile::from_bytes(&std::fs::read(p).unwrap(), "hirsch").unwrap()
}

fn out_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/difftest").join(name);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn roundtrip_is_lossless() {
    let pf = fixture();
    let bytes = pf.to_bytes().unwrap();
    let again = PopFile::from_bytes(&bytes, "hirsch").unwrap();
    assert_eq!(pf.data, again.data);
    for sp in ["red_deer", "fallow_deer", "wild_boar"] {
        assert_eq!(pf.stats(sp).unwrap(), again.stats(sp).unwrap());
    }
}

/// Same operations as `tools/apc_harness.py op ...`; outputs compared by tools/difftest.sh
#[test]
fn write_difftest_outputs() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let mut pf = fixture();
    let before = pf.stats("red_deer").unwrap();
    assert_eq!(pf.add_animals("red_deer", 1, true, &mut rng).unwrap(), 1);
    let after = pf.stats("red_deer").unwrap();
    assert_eq!(after.total, before.total + 1);
    assert_eq!(after.males, before.males + 1);
    std::fs::write(out_dir("add").join("animal_population_0"), pf.to_bytes().unwrap()).unwrap();

    let mut pf = fixture();
    let groups_before: Vec<usize> = (0..3).map(|g| pf.animals("fallow_deer").unwrap().iter().filter(|a| a.group == g).count()).collect();
    // APC removes from a specific group; ours round-robins from group 0, so remove from group 0 here
    assert_eq!(pf.remove_animals("fallow_deer", 1, false).unwrap(), 1);
    let g0 = pf.animals("fallow_deer").unwrap().iter().filter(|a| a.group == 0).count();
    assert_eq!(g0, groups_before[0] - 1);
    std::fs::write(out_dir("remove").join("animal_population_0"), pf.to_bytes().unwrap()).unwrap();
}

#[test]
fn many_inserts_and_removes_stay_consistent() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(2);
    let mut pf = fixture();
    let s0 = pf.stats("eu_bison").unwrap();
    let added = pf.add_animals("eu_bison", 40, false, &mut rng).unwrap();
    assert_eq!(added, 40);
    let removed = pf.remove_animals("eu_bison", 25, true).unwrap();
    assert!(removed > 0);
    let s1 = pf.stats("eu_bison").unwrap();
    assert_eq!(s1.females, s0.females + 40);
    assert_eq!(s1.males, s0.males - removed);
    // other species untouched
    let orig = fixture();
    for sp in ["red_deer", "wild_boar", "canada_goose"] {
        assert_eq!(pf.stats(sp).unwrap(), orig.stats(sp).unwrap());
    }
    // trailer/strings after the edits still read correctly
    let root = &pf.adf.roots[0];
    assert!(matches!(&root.get("Title").unwrap().val, cotwmod::adf::Val::Str(s) if s == "synthetic population"));
    let bytes = pf.to_bytes().unwrap();
    std::fs::write(out_dir("many").join("animal_population_0"), &bytes).unwrap();
    PopFile::from_bytes(&bytes, "hirsch").unwrap();
}

#[test]
fn group_cap_respected() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(3);
    let mut pf = fixture();
    let groups = pf.stats("red_fox").unwrap().groups;
    let total0 = pf.stats("red_fox").unwrap().total;
    let added = pf.add_animals("red_fox", 10_000, true, &mut rng).unwrap();
    assert_eq!(total0 + added, groups * cotwmod::population::MAX_GROUP);
}

#[test]
fn rarity_ops() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(4);
    let mut pf = fixture();
    let n = pf.make_diamonds("red_deer", 3, true, &mut rng).unwrap();
    assert_eq!(n, 3);
    assert_eq!(pf.stats("red_deer").unwrap().diamonds, 3);
    let g = pf.make_great_ones("red_deer", 2, &mut rng).unwrap();
    assert_eq!(g, 2);
    let st = pf.stats("red_deer").unwrap();
    assert_eq!(st.great_ones, 2);
    for a in pf.animals("red_deer").unwrap() {
        if a.great_one {
            assert!(a.male);
            assert_eq!(a.trophy, Trophy::GreatOne);
            assert_eq!(a.fur.as_deref(), Some("great_one_red_deer"));
        }
    }
    let r = pf.make_rare_furs("wild_boar", 4, &mut rng).unwrap();
    assert_eq!(r, 4);
    assert!(pf.stats("wild_boar").unwrap().rare_furs >= 4);
    let s0 = pf.stats("fallow_deer").unwrap();
    let c = pf.convert_gender("fallow_deer", 5, true, &mut rng).unwrap();
    let s1 = pf.stats("fallow_deer").unwrap();
    assert_eq!(s1.males, s0.males + c);
    let tgt = s1.females + 7;
    pf.set_count("fallow_deer", false, tgt, &mut rng).unwrap();
    assert_eq!(pf.stats("fallow_deer").unwrap().females, tgt);
    std::fs::write(out_dir("rarity").join("animal_population_0"), pf.to_bytes().unwrap()).unwrap();
}

#[test]
fn fur_seed_roundtrip() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(5);
    for fur in cotwmod::data::furs("red_deer", "male", false) {
        let seed = cotwmod::data::find_fur_seed("red_deer", "male", false, Some(&fur), &mut rng).unwrap();
        assert_eq!(cotwmod::data::fur_for_seed(seed, "red_deer", "male", false).as_deref(), Some(fur.as_str()));
    }
}
