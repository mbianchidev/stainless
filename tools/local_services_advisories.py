"""Fail-closed native source advisory checks, including Git build references."""

import hashlib
import json
import re
import subprocess
import time


def version_in_range(version, expression):
    def numeric(value):
        if not isinstance(value, str) or not re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", value):
            raise ValueError(f"Unreviewed advisory version syntax: {value}")
        values = tuple(int(part) for part in value.split("."))
        return values + (0,) * (4 - len(values))

    actual = numeric(version)
    if not isinstance(expression, str) or not expression:
        raise ValueError("Missing upstream advisory version range")
    for alternative in expression.split("||"):
        matches = True
        for condition in alternative.split(","):
            item = re.fullmatch(
                r"\s*(<=|>=|<|>|=)?\s*([0-9]+(?:\.[0-9]+){1,3})\s*", condition
            )
            if not item:
                raise ValueError(f"Unreviewed advisory range: {expression}")
            target = numeric(item[2])
            matches &= {
                "<": actual < target, "<=": actual <= target,
                ">": actual > target, ">=": actual >= target,
                "=": actual == target, None: actual == target,
            }[item[1]]
        if matches:
            return True
    return False


def query(url, body=None):
    command = [
        "/usr/bin/curl", "--disable", "--fail", "--show-error", "--silent",
        "--connect-timeout", "10", "--max-time", "60", "--proto", "=https",
        "--max-filesize", "16777216",
    ]
    if body is not None:
        command.extend(("--header", "Content-Type: application/json",
                        "--data", json.dumps(body)))
    else:
        command.extend(("--header", "Accept: application/vnd.github+json"))
    command.append(url)
    return json.loads(subprocess.check_output(command, text=True))


def ancestor_evidence(data, revision, url):
    if not isinstance(data, dict):
        raise ValueError(f"Invalid fixed-commit ancestry response: {url}")
    base = data.get("base_commit", {}).get("sha")
    merge = data.get("merge_base_commit", {}).get("sha")
    if (
        data.get("status") not in ("ahead", "identical") or
        not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base) or
        merge != base
    ):
        raise ValueError(f"Pinned source does not prove fixed-commit ancestry: {url}")
    return {"source_revision": revision, "fixed_commit": base,
            "status": data["status"], "source": url}


def disposition(entry, advisory, repository, comparisons):
    identifier = advisory.get("ghsa_id")
    vulnerabilities = advisory.get("vulnerabilities")
    if (
        not isinstance(vulnerabilities, list) or not vulnerabilities or
        any(not isinstance(item, dict) for item in vulnerabilities)
    ):
        raise ValueError(f"Missing upstream affected range: {identifier}")
    result = {
        "id": identifier, "version": entry["version"],
        "vulnerabilities": vulnerabilities,
    }
    exclusion = entry.get("configuration_advisory_exclusions", {}).get(identifier)
    if exclusion:
        description = advisory.get("description")
        if (
            not isinstance(description, str) or
            hashlib.sha256(description.encode()).hexdigest() !=
                exclusion["description_sha256"]
        ):
            raise ValueError(f"Reviewed advisory scope changed: {identifier}")
        result.update({
            "affected": False,
            "pending_configuration": exclusion["required_options"],
            "reason": exclusion["reason"],
            "source": exclusion["source"],
        })
        return result
    if entry.get("advisory_revision_scheme") != "github-build-ref":
        result["affected"] = any(
            version_in_range(entry["version"], item.get("vulnerable_version_range"))
            for item in vulnerabilities
        )
        return result
    proofs = []
    for vulnerability in vulnerabilities:
        fixed = vulnerability.get("patched_versions")
        match = re.fullmatch(r"\s*(?:>=\s*)?(b[0-9]+|[0-9a-f]{7,40})\s*", fixed or "")
        if not match:
            raise ValueError(f"Unreviewed fixed Git reference: {identifier}: {fixed}")
        reference = match[1]
        url = (
            f"https://api.github.com/repos/{repository}/compare/"
            f"{reference}...{entry['revision']}?per_page=1"
        )
        if url not in comparisons:
            comparisons[url] = ancestor_evidence(query(url), entry["revision"], url)
        proofs.append(comparisons[url])
    result.update({"affected": False, "fixed_ancestry": proofs})
    return result


def verify_disabled_advisories(evidence, options):
    for entry in evidence["inputs"].values():
        for item in entry.get("upstream", {}).get("dispositions", []):
            required = item.get("pending_configuration")
            if required is None:
                continue
            if not required or any(options.get(key) != value for key, value in required.items()):
                raise ValueError(f"Affected optional feature is not proven disabled: {item['id']}")
            item["verified_disabled_options"] = dict(required)
            del item["pending_configuration"]


def ensure_configurations_verified(evidence):
    for entry in evidence["inputs"].values():
        for item in entry.get("upstream", {}).get("dispositions", []):
            if "pending_configuration" in item:
                raise ValueError(f"Unverified optional-feature advisory: {item['id']}")


def check_source_advisories(manifest, cache, upstream_components):
    evidence = {"checked_at_unix": int(time.time()),
                "source": "https://api.osv.dev/v1/query", "inputs": {}}
    evidence["embedded_coverage"] = {
        name: {
            "version": entry["version"], "sha256": entry["sha256"],
            "parent_revision": manifest["sources"][entry["parent"]]["revision"],
            "coverage": entry["advisory_coverage"],
        }
        for name, entry in manifest.get("embedded_sources", {}).items()
        if "advisory_coverage" in entry
    }
    inputs = dict(manifest["sources"])
    inputs.update({
        name: entry for name, entry in manifest.get("embedded_sources", {}).items()
        if "revision" in entry or name in upstream_components
    })
    comparisons = {}
    for name, entry in inputs.items():
        if "revision" in entry:
            data = query(evidence["source"], {"commit": entry["revision"]})
            if not isinstance(data, dict) or not isinstance(data.get("vulns", []), list):
                raise ValueError(f"Invalid source advisory response for {name}")
            evidence["inputs"][name] = {"revision": entry["revision"], "response": data}
        else:
            evidence["inputs"][name] = {
                "parent_revision": manifest["sources"][entry["parent"]]["revision"],
                "source_sha256": entry["sha256"],
                "coverage": entry["advisory_coverage"],
                "osv_standalone_commit": "unknown; parent commit queried separately",
            }
        if name not in upstream_components:
            continue
        url = entry.get("url") or entry.get("repository")
        if not isinstance(url, str):
            raise ValueError(f"Missing official advisory repository for {name}")
        match = re.match(r"https://(?:codeload\.github\.com|github\.com)/([^/]+/[^/]+)/", url)
        if not match:
            raise ValueError(f"Missing official advisory source for {name}")
        endpoint = "https://api.github.com/repos/" + match[1] + "/security-advisories"
        advisories = []
        page = 1
        while True:
            current = query(f"{endpoint}?per_page=100&page={page}")
            if not isinstance(current, list):
                raise ValueError(f"Invalid official advisory response for {name}")
            if any(not isinstance(item, dict) for item in current):
                raise ValueError(f"Invalid official advisory entry for {name}")
            advisories.extend(current)
            if len(current) < 100:
                break
            page += 1
            if page > 100:
                raise ValueError(f"Unbounded upstream advisory pagination: {name}")
        records = [disposition(entry, item, match[1], comparisons) for item in advisories]
        if any(not isinstance(item.get("description", ""), str) for item in advisories):
            raise ValueError(f"Invalid official advisory description for {name}")
        summaries = [{
            "id": item["ghsa_id"], "published_at": item.get("published_at"),
            "updated_at": item.get("updated_at"),
            "description_sha256": hashlib.sha256(
                item.get("description", "").encode()
            ).hexdigest(),
        } for item in advisories]
        evidence["inputs"][name]["upstream"] = {
            "source": endpoint, "advisories": summaries, "dispositions": records,
        }
    path = cache / "source-advisories.json"
    path.write_text(json.dumps(evidence, indent=2) + "\n")
    affected = {
        name: [item["id"] for item in record.get("response", {}).get("vulns", [])]
        for name, record in evidence["inputs"].items()
        if record.get("response", {}).get("vulns")
    }
    for name, record in evidence["inputs"].items():
        ids = [
            item["id"] for item in record.get("upstream", {}).get("dispositions", [])
            if item["affected"]
        ]
        if ids:
            affected.setdefault(name, []).extend(ids)
    if affected:
        raise ValueError(f"Source advisory matches block the build; inspect {path}: {affected}")
    return evidence
