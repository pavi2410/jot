use crate::models::MavenVersioning;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionRangeInterval {
    pub(crate) lower: Option<(String, bool)>,
    pub(crate) upper: Option<(String, bool)>,
}

impl VersionRangeInterval {
    pub(crate) fn matches(&self, version: &str) -> bool {
        let lower_matches = self.lower.as_ref().is_none_or(|(bound, inclusive)| {
            if *inclusive {
                compare_maven_versions(version, bound).is_ge()
            } else {
                compare_maven_versions(version, bound).is_gt()
            }
        });
        let upper_matches = self.upper.as_ref().is_none_or(|(bound, inclusive)| {
            if *inclusive {
                compare_maven_versions(version, bound).is_le()
            } else {
                compare_maven_versions(version, bound).is_lt()
            }
        });

        lower_matches && upper_matches
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VersionToken {
    Number(u64),
    Text(String),
}

pub(crate) fn resolve_best_version(versioning: &MavenVersioning) -> Option<String> {
    if let Some(release) = &versioning.release
        && is_stable_maven_version(release)
    {
        return Some(release.clone());
    }

    if let Some(latest) = &versioning.latest
        && is_stable_maven_version(latest)
    {
        return Some(latest.clone());
    }

    if let Some(stable) = versioning.versions.as_ref().and_then(|versions| {
        versions
            .version
            .iter()
            .rev()
            .find(|version| is_stable_maven_version(version))
            .cloned()
    }) {
        return Some(stable);
    }

    versioning
        .versions
        .as_ref()
        .and_then(|versions| versions.version.last().cloned())
}

pub(crate) fn resolve_version_from_metadata(
    versioning: &MavenVersioning,
    version_spec: &str,
) -> Option<String> {
    if version_spec.eq_ignore_ascii_case("latest") {
        return versioning
            .latest
            .as_ref()
            .filter(|version| is_stable_maven_version(version))
            .cloned()
            .or_else(|| resolve_best_version(versioning));
    }

    if version_spec.eq_ignore_ascii_case("release") {
        return resolve_best_version(versioning);
    }

    let intervals = parse_version_spec(version_spec)?;
    let versions = versioning.versions.as_ref()?.version.as_slice();

    versions
        .iter()
        .rev()
        .find(|version| {
            is_stable_maven_version(version) && version_matches_any_range(version, &intervals)
        })
        .cloned()
        .or_else(|| {
            versions
                .iter()
                .rev()
                .find(|version| version_matches_any_range(version, &intervals))
                .cloned()
        })
}

pub(crate) fn parse_version_spec(spec: &str) -> Option<Vec<VersionRangeInterval>> {
    if !needs_dynamic_version_resolution(spec) || is_property_version_expression(spec) {
        return None;
    }

    if spec.starts_with('[') || spec.starts_with('(') {
        return parse_version_ranges(spec);
    }

    Some(vec![VersionRangeInterval {
        lower: Some((spec.to_owned(), true)),
        upper: Some((spec.to_owned(), true)),
    }])
}

fn parse_version_ranges(spec: &str) -> Option<Vec<VersionRangeInterval>> {
    let mut intervals = Vec::new();
    let mut start = 0;
    let chars = spec.char_indices().collect::<Vec<_>>();

    while start < spec.len() {
        let opening = spec[start..].chars().next()?;
        if opening != '[' && opening != '(' {
            return None;
        }

        let mut end = None;
        for (index, ch) in chars.iter().copied().filter(|(index, _)| *index > start) {
            if ch == ']' || ch == ')' {
                end = Some((index, ch));
                break;
            }
        }
        let (end_index, closing) = end?;
        let body = &spec[start + 1..end_index];
        let parts = body.splitn(2, ',').collect::<Vec<_>>();
        let lower = parts.first().copied().unwrap_or_default().trim();
        let upper = parts.get(1).copied().unwrap_or(lower).trim();

        let interval = if parts.len() == 1 {
            VersionRangeInterval {
                lower: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), opening == '['))
                },
                upper: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), closing == ']'))
                },
            }
        } else {
            VersionRangeInterval {
                lower: if lower.is_empty() {
                    None
                } else {
                    Some((lower.to_owned(), opening == '['))
                },
                upper: if upper.is_empty() {
                    None
                } else {
                    Some((upper.to_owned(), closing == ']'))
                },
            }
        };
        intervals.push(interval);

        start = end_index + 1;
        while spec[start..].starts_with(',') {
            start += 1;
            if start >= spec.len() {
                break;
            }
        }
    }

    if intervals.is_empty() {
        None
    } else {
        Some(intervals)
    }
}

pub(crate) fn version_matches_any_range(version: &str, intervals: &[VersionRangeInterval]) -> bool {
    intervals.iter().any(|interval| interval.matches(version))
}

pub(crate) fn needs_dynamic_version_resolution(version: &str) -> bool {
    version.eq_ignore_ascii_case("latest")
        || version.eq_ignore_ascii_case("release")
        || version.starts_with('[')
        || version.starts_with('(')
}

pub(crate) fn is_property_version_expression(version: &str) -> bool {
    version.contains("${")
}

pub(crate) fn is_stable_maven_version(version: &str) -> bool {
    let lowered = version.to_ascii_lowercase();
    !lowered.contains("snapshot")
        && !lowered.contains("alpha")
        && !lowered.contains("beta")
        && !lowered.contains("rc")
        && !lowered.contains("milestone")
        && !lowered.contains("m")
}

pub(crate) fn compare_maven_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = tokenize_maven_version(left);
    let right_parts = tokenize_maven_version(right);
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left_part = left_parts
            .get(index)
            .cloned()
            .unwrap_or(VersionToken::Number(0));
        let right_part = right_parts
            .get(index)
            .cloned()
            .unwrap_or(VersionToken::Number(0));
        let ordering = left_part.cmp(&right_part);
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }

    std::cmp::Ordering::Equal
}

fn tokenize_maven_version(version: &str) -> Vec<VersionToken> {
    version
        .split(['.', '-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u64>()
                .map(VersionToken::Number)
                .unwrap_or_else(|_| VersionToken::Text(part.to_ascii_lowercase()))
        })
        .collect()
}
