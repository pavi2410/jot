package com.jot.samples.mixed;

@SuppressWarnings("PMD.ShortClassName")
public class Main {

  @SuppressWarnings("PMD.SystemPrintln")
  public static void main(final String[] args) {
    final String name = args.length > 0 ? args[0] : "World";
    final Greeter greeter = new Greeter(name);
    System.out.println(greeter.greet());
  }
}
