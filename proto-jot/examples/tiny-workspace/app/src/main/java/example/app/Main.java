package example.app;

import example.greeting.Greeting;

public final class Main {
    private Main() {}

    public static void main(String[] args) {
        System.out.println(Greeting.message());
    }
}
