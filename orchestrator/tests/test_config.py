from synapse_os.config import Config


def test_default_config():
    cfg = Config()
    assert cfg.redis_url == "redis://localhost:6379/0"
    assert cfg.spacetimedb_base_url == "http://localhost:3000"
    assert cfg.spacetimedb_module == "synapse-backend-g9cee"
    assert cfg.standup_interval_seconds == 86400
    assert cfg.heartbeat_timeout_seconds == 60
    assert cfg.blocked_threshold_seconds == 1800
    assert cfg.feedback_poll_interval_seconds == 5


def test_config_from_env(monkeypatch):
    monkeypatch.setenv("SYNAPSE_REDIS_URL", "redis://custom:6380/1")
    monkeypatch.setenv("SYNAPSE_STANDUP_INTERVAL", "3600")
    cfg = Config.from_env()
    assert cfg.redis_url == "redis://custom:6380/1"
    assert cfg.standup_interval_seconds == 3600
