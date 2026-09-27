export function discoverExpressRoutes(source, relative) {
  const routes = []
  const declaration = /app\.(get|post|put|patch|delete|options)\(\s*(\[[\s\S]*?\]|(['"])(.*?)\3)\s*,/g
  for (const match of source.matchAll(declaration)) {
    const line = source.slice(0, match.index).split('\n').length
    const paths = match[2].startsWith('[')
      ? [...match[2].matchAll(/(['"])(.*?)\1/g)].map((item) => item[2])
      : [match[4]]
    for (const path of paths) routes.push({ method: match[1].toUpperCase(), path, source: `${relative}:${line}` })
  }
  return routes
}
