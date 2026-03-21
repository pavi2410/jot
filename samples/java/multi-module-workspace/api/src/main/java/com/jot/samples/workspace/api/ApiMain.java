package com.jot.samples.workspace.api;

import com.jot.samples.workspace.domain.Order;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;

public final class ApiMain {
    public static void main(String[] args) throws IOException {
        HttpServer server = HttpServer.create(new InetSocketAddress(8080), 0);
        server.createContext("/health", exchange -> {
            Order order = new Order("A-1", "jot");
            byte[] body = ("ok:" + order.id()).getBytes();
            exchange.sendResponseHeaders(200, body.length);
            try (OutputStream output = exchange.getResponseBody()) {
                output.write(body);
            }
        });
        server.start();
        System.out.println("shopflow api listening on :8080");
    }
}
