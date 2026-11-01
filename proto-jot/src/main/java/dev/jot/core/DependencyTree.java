package dev.jot.core;

import dev.jot.core.resolve.MavenResolver.TreeEntry;
import java.util.List;

/** Renders the flattened resolver graph as a readable dependency tree. */
public final class DependencyTree {
  private DependencyTree() {}

  public static String render(List<TreeEntry> entries) {
    if (entries.isEmpty()) return "";
    return render(toNode(entries));
  }

  public static String renderWorkspace(List<WorkspaceModule> modules) {
    TreeNode root = new TreeNode("workspace");
    for (WorkspaceModule module : modules) {
      TreeNode moduleNode = root.add(new TreeNode(module.name()));
      for (String dependency : module.workspaceDependencies()) {
        moduleNode.add(new TreeNode(dependency + " (workspace)"));
      }
      for (List<TreeEntry> entries : module.externalTrees()) {
        if (!entries.isEmpty()) moduleNode.add(toNode(entries));
      }
    }
    return render(root);
  }

  private static TreeNode toNode(List<TreeEntry> entries) {
    TreeNode root = new TreeNode(entries.getFirst().coordinate().notation());
    List<TreeNode> parents = new java.util.ArrayList<>(List.of(root));
    for (int i = 1; i < entries.size(); i++) {
      TreeEntry entry = entries.get(i);
      String label = entry.coordinate().notation() + " [" + entry.scope();
      if (entry.optional()) label += " optional";
      label += "]";
      if (entry.note() != null) label += " " + entry.note();
      TreeNode node = new TreeNode(label);
      TreeNode parent = parents.get(entry.depth() - 1);
      parent.add(node);
      while (parents.size() > entry.depth()) parents.removeLast();
      parents.add(node);
    }
    return root;
  }

  private static String render(TreeNode root) {
    StringBuilder output = new StringBuilder(root.label());
    append(root, "", output);
    return output.append(System.lineSeparator()).toString();
  }

  private static void append(TreeNode parent, String prefix, StringBuilder output) {
    for (int i = 0; i < parent.children.size(); i++) {
      TreeNode child = parent.children.get(i);
      boolean last = i == parent.children.size() - 1;
      output
          .append(System.lineSeparator())
          .append(prefix)
          .append(last ? "└── " : "├── ")
          .append(child.label());
      append(child, prefix + (last ? "    " : "│   "), output);
    }
  }

  public record WorkspaceModule(
      String name, List<String> workspaceDependencies, List<List<TreeEntry>> externalTrees) {}

  private static final class TreeNode {
    private final String label;
    private final List<TreeNode> children = new java.util.ArrayList<>();

    private TreeNode(String label) {
      this.label = label;
    }

    private String label() {
      return label;
    }

    private TreeNode add(TreeNode child) {
      children.add(child);
      return child;
    }
  }
}
