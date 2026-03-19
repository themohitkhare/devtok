import pytest
from click.testing import CliRunner
from synapse_os.cli import cli


def test_cli_help():
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"])
    assert result.exit_code == 0
    assert "init" in result.output
    assert "run" in result.output
    assert "status" in result.output


def test_init_requires_path():
    runner = CliRunner()
    result = runner.invoke(cli, ["init"])
    assert result.exit_code != 0


def test_run_requires_path():
    runner = CliRunner()
    result = runner.invoke(cli, ["run"])
    assert result.exit_code != 0
