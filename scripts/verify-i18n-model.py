#!/usr/bin/env python3
"""Proof of actual (non-fixture) model replies using Fidget's real Director prompt.

Run on GitHub's native Ubuntu runner against a real local Ollama model.
This is an inference test, NOT a fake ACP response or a translation-catalog test.
"""
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from urllib.error import URLError
from urllib.request import Request, urlopen

from langdetect import DetectorFactory, detect_langs

DetectorFactory.seed = 0
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/i18n/model-proof"
MODEL = os.environ.get("FIDGET_PROOF_MODEL", "qwen2.5:7b")
URL = os.environ.get("FIDGET_PROOF_URL", "http://127.0.0.1:11434/v1/chat/completions")
LOCALES = {"en": "English", "fr": "French", "es": "Spanish"}


def wire(prompt):
    body = {
        "model": MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "stream": False,
        "temperature": 0,
        "max_tokens": 200,
    }
    req = Request(URL, data=json.dumps(body).encode("utf-8"), method="POST",
                  headers={"Content-Type": "application/json"})
    with urlopen(req, timeout=180) as response:
        payload = json.load(response)
    return payload["choices"][0]["message"]["content"]


def spoken_line(raw):
    # A Fidget reply normally starts with the (English) behavior identifier,
    # then the spoken text. Do not send the fixed behavior token to langdetect.
    lines = raw.strip().splitlines()
    if not lines:
        raise AssertionError("the model returned no content")
    marker = re.compile(r"^\s*(?:behavior:\s*)?(?:wave|nap|sit)\s*$", re.I)
    if marker.fullmatch(lines[0]):
        lines = lines[1:]
    speech = "\n".join(lines).strip()
    if not speech:
        raise AssertionError(f"the model returned only a behavior: {raw!r}")
    if len(re.findall(r"[^\W\d_]", speech, flags=re.UNICODE)) < 22:
        raise AssertionError(f"the spoken reply is too short for language detection: {speech!r}")
    return speech


def probe(lang):
    command = [str(ROOT / "target/debug/examples/i18n-model-probe"), lang]
    prompt = subprocess.check_output(command, text=True)
    expected_rule = "Write every spoken line in " + LOCALES[lang] + "."
    if lang != "en" and expected_rule not in prompt:
        raise AssertionError(f"{lang}: the Fidget prompt omitted {expected_rule!r}")
    if lang == "en" and "Write every spoken line in" in prompt:
        raise AssertionError("the English setting unexpectedly inserted a foreign-language rule")
    reply = wire(prompt)
    failure = None
    try:
        speech = spoken_line(reply)
        best = detect_langs(speech)[0]
        detected, confidence = best.lang, best.prob
    except (AssertionError, ValueError) as exc:
        # The unedited answer is evidence even if the model ignores the
        # response contract. Continue to test the other languages.
        speech, detected, confidence = reply.strip(), "unclassified", 0.0
        failure = str(exc)
    # A long French/Spanish paragraph can mask a short English line
    # ("BMO on the first line") in a whole-response language score.
    # Inspect every substantive spoken line separately.
    line_checks = []
    for line in speech.splitlines():
        line = line.strip()
        if sum(ch.isalpha() for ch in line) < 12:
            continue
        try:
            found = detect_langs(line)[0]
            line_checks.append({
                "text": line,
                "detected": found.lang,
                "confidence": round(found.prob, 4),
                "foreign": found.lang != lang and found.prob >= 0.80,
            })
        except Exception as exc:
            line_checks.append({"text": line, "error": str(exc), "foreign": True})
    mixed_language = any(item["foreign"] for item in line_checks)
    result = {
        "setting": lang,
        "language": LOCALES[lang],
        "model": MODEL,
        "request_url": URL,
        "fidget_prompt": prompt,
        "raw_model_response": reply,
        "spoken_text": speech,
        "detected_language": detected,
        "detector_confidence": round(confidence, 4),
        "passed": failure is None and detected == lang and confidence >= 0.80 and not mixed_language,
        "line_checks": line_checks,
        "failure_reason": failure or ("mixed-language spoken lines" if mixed_language else None),
    }
    print(f"{lang} → {detected} ({confidence:.3f}): {speech!r}", flush=True)
    return result


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        for lang in LOCALES:
            results.append(probe(lang))
        if not all(r["passed"] for r in results):
            raise AssertionError("at least one live model reply violated the selected language")
    finally:
        # Keep verbatim evidence even on failure; CI uploads it as a run artifact.
        (OUT / "results.json").write_text(json.dumps({
            "timestamp_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "test_type": "actual local LLM inference (not mocked)",
            "model": MODEL,
            "results": results,
        }, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        lines = [
            "# Live model language proof",
            "",
            f"**Model:** `{MODEL}` (local Ollama, GitHub-hosted Ubuntu runner).",
            "**Method:** The Rust helper calls Fidget's actual `app_instructions()`.",
            "The same OpenAI-compatible chat endpoint as Fidget's Model API receives",
            "a BMO character test turn. The unmodified model reply is recorded;",
            "the spoken portion is checked with `langdetect` (confidence >= 0.80).",
            "",
            "No canned speech, no secret API key and no translated test answers.",
            "This verifies this model and these turns, not a universal guarantee",
            "for every model, prompt or future response.",
            "",
            "| Selected language | Detected | Confidence | Result | Actual spoken response |",
            "| --- | --- | ---: | --- | --- |",
        ]
        for row in results:
            speech = row["spoken_text"].replace("|", "\\|").replace("\n", " ")
            lines.append(
                f'| {row["language"]} | {row["detected_language"]} | '
                f'{row["detector_confidence"]:.3f} | '
                f'{"PASS" if row["passed"] else "FAIL"} | {speech} |'
            )
        lines += [
            "",
            "The full request and unedited responses are in [results.json](results.json).",
            "The real app and picker screenshots are in the",
            "[language gallery](../gallery/README.md). The French BMO bubble there",
            "is a deterministic ACP fixture and is **not** this live-model test.",
            "",
            "**Reproduce:** [i18n-model-proof GitHub Actions workflow](../../../.github/workflows/i18n-model-proof.yml).",
            "",
        ]
        (OUT / "README.md").write_text("\n".join(lines), encoding="utf-8")
    print("PASS: the local model spoke English, French and Spanish on the matching settings", flush=True)


if __name__ == "__main__":
    main()
