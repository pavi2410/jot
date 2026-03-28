package com.jot.samples.workspace.cli;

import com.jot.samples.workspace.domain.Order;

public final class CliMain {

  private CliMain() {}

  @SuppressWarnings("PMD.SystemPrintln")
  public static void main(final String[] args) {
    if (args.length > 0 && "--help".equals(args[0])) {
      System.out.println("usage: shopflow-cli [customer]");
      return;
    }

    final String customer = args.length > 0 ? args[0] : "jot";
    final Order order = new Order("A-1", customer);
    System.out.println("generated order for " + order.customer());
  }
}
