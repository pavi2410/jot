package example;

import org.openjdk.jmh.annotations.Benchmark;
import org.openjdk.jmh.annotations.Scope;
import org.openjdk.jmh.annotations.State;

@State(Scope.Thread)
public class CounterBenchmark {
  private final Counter counter = new Counter();

  @Benchmark
  public int increment() {
    return counter.increment(41);
  }
}
