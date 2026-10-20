@echo off
rem The ACP fixture launcher for Windows, twin of fixture-harness.sh.
rem Preflight runs `<launcher> --version`, which the Rust test binary rejects.
rem %* keeps script=/count= whole (cmd splits %2 at '='); they become extra
rem libtest filters, and --exact matches none of them.
if "%~1"=="--version" (
  echo fixture-harness
  exit /b 0
)
%* harness::tests::fake_acp_agent --exact --nocapture --test-threads=1
