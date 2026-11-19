type Point = { x: number; y: number };

export function translate(point: Point, delta: Point): Point {
  // Template strings and Markdown fences remain source text.
  const hint = `Move to ${point.x}, ${point.y}`;
  console.log(hint, "```markdown");
  return {
    x: point.x + delta.x,
    y: point.y + delta.y,
  };
}
