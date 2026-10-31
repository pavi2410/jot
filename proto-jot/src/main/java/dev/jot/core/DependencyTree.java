package dev.jot.core;

import dev.jot.core.resolve.MavenResolver.TreeEntry;
import java.util.List;

/** Renders the flattened resolver graph as a readable dependency tree. */
public final class DependencyTree {
    private DependencyTree() {}

    public static String render(List<TreeEntry> entries) {
        if (entries.isEmpty()) return "";
        StringBuilder output = new StringBuilder(entries.getFirst().coordinate().notation());
        appendChildren(entries, 1, 0, "", output);
        return output.append(System.lineSeparator()).toString();
    }

    private static int appendChildren(List<TreeEntry> entries, int cursor, int parentDepth,
                                      String prefix, StringBuilder output) {
        int end = cursor;
        while (end < entries.size() && entries.get(end).depth() > parentDepth) end++;
        int childCount = 0;
        for (int i = cursor; i < end; i++) {
            if (entries.get(i).depth() == parentDepth + 1) childCount++;
        }
        int childIndex = 0;
        while (cursor < end) {
            TreeEntry entry = entries.get(cursor);
            if (entry.depth() != parentDepth + 1) {
                cursor++;
                continue;
            }
            boolean last = ++childIndex == childCount;
            output.append(System.lineSeparator()).append(prefix).append(last ? "└── " : "├── ")
                    .append(entry.coordinate().notation()).append(" [").append(entry.scope());
            if (entry.optional()) output.append(" optional");
            output.append(']');
            if (entry.note() != null) output.append(' ').append(entry.note());
            String childPrefix = prefix + (last ? "    " : "│   ");
            cursor = appendChildren(entries, cursor + 1, entry.depth(), childPrefix, output);
        }
        return end;
    }
}
