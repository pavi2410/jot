package dev.jot.core;

import dev.jot.core.resolve.Lockfile;
import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver;
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

    public static String renderOutdated(ProjectConfig config, ProjectWorkspace workspace,
                                        String module, Path lockPath, MavenResolver resolver)
            throws Exception {
        Lockfile lock = Lockfile.read(lockPath);
        Set<String> included = null;
        if (module != null) {
            if (!config.isWorkspace()) throw new IllegalArgumentException("--module requires a workspace");
            WorkspaceProject project = workspace.find(module);
            included = new HashSet<>();
            List<String> roots = new ArrayList<>(project.config().dependencies().values());
            roots.addAll(project.config().testDependencies().values());
            for (String root : roots) {
                for (MavenResolver.TreeEntry entry : resolver.resolveDependencyTree(
                        MavenCoordinate.parse(root), 8)) {
                    if (entry.note() == null) included.add(entry.coordinate().notation());
                }
            }
        }

        Set<String> selectedCoordinates = included;
        List<Lockfile.LockedPackage> packages = lock.packages().stream()
                .filter(item -> selectedCoordinates == null
                        || selectedCoordinates.contains(item.coordinate().notation()))
                .toList();
        if (packages.isEmpty()) return "No locked packages found.\n";

        List<String> results = new ArrayList<>();
        for (Lockfile.LockedPackage item : packages) {
            MavenCoordinate coordinate = item.coordinate();
            var latest = resolver.latestAvailableVersion(coordinate).orElse(null);
            if (latest == null) {
                results.add(coordinate.group() + ":" + coordinate.artifact() + " "
                        + coordinate.version() + " -> <unknown>");
            } else if (!latest.equals(coordinate.version())) {
                results.add(coordinate.group() + ":" + coordinate.artifact() + " "
                        + coordinate.version() + " -> " + latest);
            }
        }
        if (results.isEmpty()) return "All locked packages are up to date.\n";
        return String.join("\n", results) + "\n";
    }

    private record Entry(String module, String scope, String name, String coordinate, boolean locked) {}
}
