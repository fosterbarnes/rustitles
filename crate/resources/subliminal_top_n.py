# Save up to N subtitles for one video. Subliminal's own CLI keeps one per language.
# rustitles runs this with `python -c` and passes settings as JSON in RUSTITLES_DRIVER_CONFIG.
# The best match per language keeps Subliminal's name (Name.en.srt); extras get Name.en.2.srt, ...
import hashlib
import json
import os
import re
import sys

try:
    from babelfish import Language
    from subliminal import (
        compute_score,
        download_subtitles,
        get_scores,
        list_subtitles,
        refine,
        region,
        scan_video,
    )
    from subliminal.core import search_external_subtitles
except ImportError as error:
    print(f"No module named subliminal ({error})", file=sys.stderr)
    sys.exit(3)


def say(line):
    print(line, flush=True)


def fingerprint(subtitle):
    text = re.sub(r"\s+", " ", subtitle.text or "").strip().lower()
    return hashlib.sha1(text.encode("utf-8", "replace")).hexdigest()


def is_valid(subtitle):
    if not subtitle.content:
        return False
    check = getattr(subtitle, "is_valid", None)
    try:
        return check() if callable(check) else True
    except Exception:
        return False


def main():
    config = json.loads(os.environ["RUSTITLES_DRIVER_CONFIG"])
    count = max(1, int(config["count"]))
    if not getattr(region, "is_configured", False):
        region.configure("dogpile.cache.memory")

    video = scan_video(config["video"])
    refine(video, refiners=config["refiners"] or None)

    languages = {Language.fromietf(code) for code in config["languages"]}
    if not config["force"]:
        # Like the CLI: existing sidecar files (Name.en.srt) count as present languages.
        # With SDH excluded, Name.en.sdh.srt must not stand in for a regular English subtitle.
        external = search_external_subtitles(video.name).values()
        if config["exclude_hearing_impaired"]:
            external = [s for s in external if not s.hearing_impaired]
        video.subtitles.extend(external)
        languages -= video.subtitle_languages
    if not languages:
        say("Downloaded 0 subtitles")
        return

    pool_kwargs = {
        "providers": config["providers"] or None,
        "provider_configs": config["provider_configs"],
    }
    candidates = list_subtitles({video}, languages, **pool_kwargs).get(video, [])
    if config["exclude_hearing_impaired"]:
        candidates = [s for s in candidates if not getattr(s, "hearing_impaired", False)]

    hash_score = get_scores(video)["hash"]
    min_score = hash_score * int(config["min_score"]) // 100
    scored = [(s, compute_score(s, video)) for s in candidates]
    scored = sorted((p for p in scored if p[1] >= min_score), key=lambda p: p[1], reverse=True)

    # Pass 1: the best match of each language. Pass 2: next-best of any language until N.
    target = max(count, len(languages))
    picked, seen = [], set()

    def try_download(batch):
        download_subtitles([s for s, _ in batch], **pool_kwargs)
        for subtitle, score in batch:
            if not is_valid(subtitle):
                continue
            key = (subtitle.language, fingerprint(subtitle))
            if key in seen:
                continue
            seen.add(key)
            picked.append((subtitle, score))

    remaining = list(scored)
    while remaining:
        have = {s.language for s, _ in picked}
        batch, batch_languages = [], set()
        for pair in remaining:
            language = pair[0].language
            if language not in have and language not in batch_languages:
                batch.append(pair)
                batch_languages.add(language)
        if not batch:
            need = target - len(picked)
            if need <= 0:
                break
            batch = remaining[:need]
        batch_ids = {id(s) for s, _ in batch}
        remaining = [p for p in remaining if id(p[0]) not in batch_ids]
        say(f"Downloading {len(picked) + 1} of {target} ({batch[0][0].provider_name})")
        try_download(batch)

    results, per_language = [], {}
    for subtitle, score in picked:
        index = per_language.get(subtitle.language, 0) + 1
        per_language[subtitle.language] = index
        path = subtitle.get_path(video)
        if index > 1:
            base, ext = os.path.splitext(path)
            path = f"{base}.{index}{ext}"
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(subtitle.text)
        results.append(
            {
                "path": path,
                "score": round(score * 100 / hash_score),
                "provider": subtitle.provider_name,
            }
        )
    say("RUSTITLES_RESULT " + json.dumps(results))
    say(f"Downloaded {len(results)} subtitle{'' if len(results) == 1 else 's'}")


main()
