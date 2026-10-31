package dev.jot.core;

import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.jot.core.resolve.MavenCoordinate;
import dev.jot.core.resolve.MavenResolver.TreeEntry;
import java.util.List;
import org.junit.jupiter.api.Test;

class DependencyTreeTest {
  @Test
  void rendersWorkspaceModulesWithLocalAndExternalDependencies() {
    List<TreeEntry> externalTree =
        List.of(
            new TreeEntry(0, MavenCoordinate.parse("sample:root:1.0"), "", false, null),
            new TreeEntry(1, MavenCoordinate.parse("sample:child:2.0"), "runtime", false, null));

    String rendered =
        DependencyTree.renderWorkspace(
            List.of(
                new DependencyTree.WorkspaceModule("core", List.of(), List.of()),
                new DependencyTree.WorkspaceModule("app", List.of("core"), List.of(externalTree))));

    assertEquals(
        "workspace"
            + System.lineSeparator()
            + "├── core"
            + System.lineSeparator()
            + "└── app"
            + System.lineSeparator()
            + "    ├── core (workspace)"
            + System.lineSeparator()
            + "    └── sample:root:1.0"
            + System.lineSeparator()
            + "        └── sample:child:2.0 [runtime]"
            + System.lineSeparator(),
        rendered);
  }
}
