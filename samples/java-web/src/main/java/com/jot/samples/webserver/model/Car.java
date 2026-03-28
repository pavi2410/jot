package com.jot.samples.webserver.model;

@SuppressWarnings("PMD.ShortClassName")
public class Car {

  public enum CarType {
    SEDAN,
    SUV,
    TRUCK
  }

  private String make;
  private int numberOfSeats;
  private CarType type;

  public Car(final String make, final int numberOfSeats, final CarType type) {
    this.make = make;
    this.numberOfSeats = numberOfSeats;
    this.type = type;
  }

  public String getMake() {
    return make;
  }

  public int getNumberOfSeats() {
    return numberOfSeats;
  }

  public CarType getType() {
    return type;
  }
}
