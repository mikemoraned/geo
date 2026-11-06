import lookout_evaluation as evaluation


def test_each_setting_shows_as_the_call_that_builds_it():
    settings = [
        'Oracle(horizon="5m")',
        "CrowFlies(radius_metres=5000.0)",
        'Scoring(horizon="5m", last_moment="30s", ramp="1m", last_moment_score=0.25, '
        'floor=0.05, warm_up="1m")',
        "Bootstrap(resamples=1000, seed=1)",
    ]

    assert [repr(eval(setting, vars(evaluation))) for setting in settings] == settings
