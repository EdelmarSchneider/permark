// A reactive holder for tests: a store reads `box.value` inside its effects, the test changes it.

export class Box<T> {
  value: T;

  constructor(initial: T) {
    this.value = $state.raw(initial);
  }
}
