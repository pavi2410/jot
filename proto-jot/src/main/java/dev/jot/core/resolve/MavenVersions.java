package dev.jot.core.resolve;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/** Resolves Maven's latest/release selectors and version range syntax. */
final class MavenVersions {
  private MavenVersions() {}

  static boolean isDynamic(String version) {
    return version.equalsIgnoreCase("latest")
        || version.equalsIgnoreCase("release")
        || version.startsWith("[")
        || version.startsWith("(");
  }

  static String best(String release, String latest, List<String> versions) {
    if (stable(release)) return release;
    if (stable(latest)) return latest;
    return versions.stream()
        .sorted((left, right) -> compare(right, left))
        .filter(MavenVersions::stable)
        .findFirst()
        .orElseGet(() -> versions.stream().max(MavenVersions::compare).orElse(null));
  }

  static String resolve(String spec, String release, String latest, List<String> versions) {
    if (spec.equalsIgnoreCase("latest")) {
      if (stable(latest)) return latest;
      return best(release, latest, versions);
    }
    if (spec.equalsIgnoreCase("release")) return best(release, latest, versions);
    List<Interval> intervals = parse(spec);
    if (intervals == null) return null;
    return versions.stream()
        .filter(version -> stable(version) && intervals.stream().anyMatch(i -> i.matches(version)))
        .max(MavenVersions::compare)
        .orElseGet(
            () ->
                versions.stream()
                    .filter(version -> intervals.stream().anyMatch(i -> i.matches(version)))
                    .max(MavenVersions::compare)
                    .orElse(null));
  }

  static boolean stable(String version) {
    if (version == null) return false;
    String normalized = version.toLowerCase(Locale.ROOT);
    return !normalized.contains("snapshot")
        && !normalized.contains("alpha")
        && !normalized.contains("beta")
        && !normalized.contains("rc")
        && !normalized.contains("milestone")
        && !normalized.contains("-m");
  }

  private static List<Interval> parse(String spec) {
    if (spec == null || spec.isBlank()) return null;
    List<Interval> intervals = new ArrayList<>();
    int start = 0;
    while (start < spec.length()) {
      char opening = spec.charAt(start);
      if (opening != '[' && opening != '(') return null;
      int end = -1;
      for (int i = start + 1; i < spec.length(); i++) {
        char current = spec.charAt(i);
        if (current == ']' || current == ')') {
          end = i;
          break;
        }
      }
      if (end < 0) return null;
      String body = spec.substring(start + 1, end).trim();
      String[] bounds = body.split(",", -1);
      if (bounds.length > 2) return null;
      String lower = bounds[0].trim();
      String upper = bounds.length == 1 ? lower : bounds[1].trim();
      if (bounds.length == 1 && lower.isEmpty()) return null;
      intervals.add(
          new Interval(
              lower.isEmpty() ? null : new Bound(lower, opening == '['),
              upper.isEmpty() ? null : new Bound(upper, spec.charAt(end) == ']')));
      start = end + 1;
      if (start == spec.length()) break;
      if (spec.charAt(start) != ',') return null;
      start++;
    }
    return intervals.isEmpty() ? null : intervals;
  }

  private static List<Token> tokens(String version) {
    List<Token> result = new ArrayList<>();
    for (String part : version.split("[._-]")) {
      if (part.isEmpty()) continue;
      try {
        result.add(new Token(Long.parseLong(part), null));
      } catch (NumberFormatException ignored) {
        result.add(new Token(null, part.toLowerCase(Locale.ROOT)));
      }
    }
    return result;
  }

  private static int compare(String left, String right) {
    List<Token> leftTokens = tokens(left);
    List<Token> rightTokens = tokens(right);
    int length = Math.max(leftTokens.size(), rightTokens.size());
    for (int index = 0; index < length; index++) {
      Token a = index < leftTokens.size() ? leftTokens.get(index) : new Token(0L, null);
      Token b = index < rightTokens.size() ? rightTokens.get(index) : new Token(0L, null);
      int order;
      if (a.number != null && b.number != null) order = Long.compare(a.number, b.number);
      else if (a.number != null) order = -1;
      else if (b.number != null) order = 1;
      else order = a.text.compareTo(b.text);
      if (order != 0) return order;
    }
    return 0;
  }

  private record Token(Long number, String text) {}

  private record Bound(String version, boolean inclusive) {}

  private record Interval(Bound lower, Bound upper) {
    boolean matches(String version) {
      if (lower != null) {
        int comparison = compare(version, lower.version());
        if (comparison < 0 || (comparison == 0 && !lower.inclusive())) return false;
      }
      if (upper != null) {
        int comparison = compare(version, upper.version());
        if (comparison > 0 || (comparison == 0 && !upper.inclusive())) return false;
      }
      return true;
    }
  }
}
