package com.jot.samples.webserver;

import com.jot.samples.webserver.model.Car;
import com.jot.samples.webserver.model.CarDto;
import com.jot.samples.webserver.model.CarMapper;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;

public final class ServerMain {

  private ServerMain() {}

  @SuppressWarnings("PMD.SystemPrintln")
  public static void main(final String[] args) throws IOException {
    final HttpServer server = HttpServer.create(new InetSocketAddress(8080), 0);

    server.createContext(
        "/health",
        exchange -> {
          final byte[] body = "ok".getBytes();
          exchange.sendResponseHeaders(200, body.length);
          try (OutputStream output = exchange.getResponseBody()) {
            output.write(body);
          }
        });

    server.createContext(
        "/car",
        exchange -> {
          final Car car = new Car("Toyota", 5, Car.CarType.SEDAN);
          final CarDto dto = CarMapper.INSTANCE.carToCarDto(car);
          final byte[] body = dto.toString().getBytes();
          exchange.sendResponseHeaders(200, body.length);
          try (OutputStream output = exchange.getResponseBody()) {
            output.write(body);
          }
        });

    server.start();
    System.out.println("server started on http://localhost:8080");
  }
}
