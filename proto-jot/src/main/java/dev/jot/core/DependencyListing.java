package dev.jot.core;

import dev.jot.core.resolve.Lockfile;
import dev.jot.core.resolve.MavenCoordinate;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/** Builds a stable, read-only view of direct project and workspace dependencies. */
public final class DependencyListing {
    private DependencyListing() {}

    public static String render(ProjectConfig config, ProjectWorkspace workspace, String module,
                                Path lockPath) throws IOException {
        List<WorkspaceProject> projects;
        if (config.isWorkspace()) {
            projects = module == null ? workspace.ordered() : List.of(workspace.find(module));
        } else {
            if (module != null) throw new IllegalArgumentException("--module requires a workspace");
            projects = List.of(workspace.find(config.name()));
        }

        Set<String> lockedRoots = new HashSet<>();
        if (Files.isRegularFile(lockPath)) {
            Lockfile.read(lockPath).roots().stream().map(MavenCoordinate::notation)
                    .forEach(lockedRoots::add);
        }
        List<Entry> entries = new ArrayList<>();
        for (WorkspaceProject project : projects) {
            for (var dependency : project.config().dependencies().entrySet()) {
                MavenCoordinate coordinate = MavenCoordinate.parse(dependency.getValue());
                entries.add(new Entry(project.name(), "main", dependency.getKey(),
                        coordinate.notation(), lockedRoots.contains(coordinate.notation())));
            }
            for (var dependency : project.config().pathDependencies().entrySet()) {
                entries.add(new Entry(project.name(), "path", dependency.getKey(),
                        dependency.getValue(), false));
            }
            for (var dependency : project.config().testDependencies().entrySet()) {
                MavenCoordinate coordinate = MavenCoordinate.parse(dependency.getValue());
                entries.add(new Entry(project.name(), "test", dependency.getKey(),
                        coordinate.notation(), lockedRoots.contains(coordinate.notation())));
            }
        }
        entries.sort(Comparator.comparing(Entry::module).thenComparing(Entry::scope)
                .thenComparing(Entry::name));
        if (entries.isEmpty()) return "No dependencies declared.\n";

        StringBuilder output = new StringBuilder();
        String currentModule = null;
        for (Entry entry : entries) {
            if (!entry.module().equals(currentModule)) {
                currentModule = entry.module();
                if (!output.isEmpty()) output.append('\n');
                output.append(currentModule).append(':').append('\n');
            }
            output.append("  ").append(entry.scope()).append("  ")
                    .append(entry.name()).append(" = ").append(entry.coordinate());
            if (entry.scope().equals("path")) output.append(" [local]");
            else output.append(entry.locked() ? " [locked]" : " [unlocked]");
            output.append('\n');
        }
        return output.toString();
    }

    private record Entry(String module, String scope, String name, String coordinate, boolean locked) {}
}
