import pytest

from cotwmod.population.profile import ProfileError, parse_amount, parse_count, parse_profile

VALID = {"hirsch": ["red_deer", "fallow_deer"], "layton": ["moose"]}
GO = {"red_deer", "fallow_deer", "moose"}


def test_counts():
    assert parse_count("+20").mode == "+"
    assert parse_count("-5").mode == "-" and parse_count("-5").value == 5
    assert parse_count("=40").mode == "=" and parse_count("=40").value == 40
    assert parse_count(7).mode == "+"
    assert parse_count(-3).mode == "-"
    with pytest.raises(ProfileError):
        parse_count("lots")


def test_amounts():
    assert parse_amount("10%").resolve(200) == 20
    assert parse_amount(8).resolve(200) == 8
    assert parse_amount("all").resolve(37) == 37
    with pytest.raises(ProfileError):
        parse_amount("150%")
    with pytest.raises(ProfileError):
        parse_amount(-1)


def test_order_is_fixed():
    plans = parse_profile({"reserves": {"hirsch": {"red_deer": {
        "great_ones": 1, "diamonds": "5%", "males": "+3", "rare_furs": 2}}}}, VALID, GO)
    assert [k for k, _ in plans[0].ops] == ["males", "rare_furs", "diamonds", "great_ones"]


def test_validation():
    with pytest.raises(ProfileError):
        parse_profile({"reserves": {"nowhere": {}}}, VALID, GO)
    with pytest.raises(ProfileError):
        parse_profile({"reserves": {"layton": {"red_deer": {}}}}, VALID, GO)
    with pytest.raises(ProfileError):
        parse_profile({"reserves": {"hirsch": {"red_deer": {"diamondz": 3}}}}, VALID, GO)
    with pytest.raises(ProfileError):
        parse_profile({"reserves": {"hirsch": {"red_deer": {"great_ones": 1}}}}, VALID, set())
