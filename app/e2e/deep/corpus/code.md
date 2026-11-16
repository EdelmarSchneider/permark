# Code blocks

```rust title="src/main.rs" {2,4-5}
fn main() {
    let tide = 1.2;
    println!("tide {tide}");
    let waves = [0.4, 0.6];
    println!("{waves:?}");
}
```

```python showLineNumbers {3}
def mean(values):
    """Average of a list."""
    return sum(values) / len(values)
```

```ts title="A title with <angle> & \"quotes\""
export const greeting: string = `hello ${"world"}`;
```

```
A block without a language.
```

~~~json
{ "fenced": "with tildes", "nested": "```" }
~~~

````markdown
```js
// a fence inside a fence
```
````

```text
a very long line that does not wrap in a code block and has to scroll sideways because it goes on and on and on and on and on and on
```

```unknownlanguage
Text in a language no highlighter knows.
```

```diff
- removed line
+ added line
```
