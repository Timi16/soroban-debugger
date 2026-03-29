import * as fs from 'fs'

export interface FunctionRange {
  name: string
  startLine: number
  endLine: number
}

export interface ResolvedBreakpoint {
  requestedLine: number
  line: number
  verified: boolean
  functionName?: string
  reasonCode?: string
  message?: string
  /**
   * Whether to set a runtime function breakpoint for this source breakpoint.
   * Source breakpoints can be unverified but still mapped to a function as a best-effort.
   */
  setBreakpoint?: boolean
}

export interface DiagnosticReport {
  line: number
  status: string
  reason: string
  functionName?: string
}

const FUNCTION_DECL = /^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/

export function parseFunctionRanges(sourcePath: string): FunctionRange[] {
  const source = fs.readFileSync(sourcePath, 'utf8')
  const lines = source.split(/\r?\n/)
  const ranges: FunctionRange[] = []

  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(FUNCTION_DECL)
    if (!match) {
      continue
    }

    const name = match[1]
    let bodyDepth = 0
    let bodyStarted = false
    let endLine = index + 1

    for (let cursor = index; cursor < lines.length; cursor += 1) {
      const line = lines[cursor]
      const opens = (line.match(/\{/g) || []).length
      const closes = (line.match(/\}/g) || []).length

      if (opens > 0) {
        bodyStarted = true
      }

      bodyDepth += opens - closes
      endLine = cursor + 1

      if (bodyStarted && bodyDepth <= 0) {
        break
      }
    }

    ranges.push({
      name,
      startLine: index + 1,
      endLine,
    })
  }

  return ranges
}

export function shouldPromoteToFunctionBreakpoint(
  verified: boolean,
  functionName?: string,
  reasonCode?: string
): boolean {
  if (!functionName) return false
  if (verified) return true
  return reasonCode === 'HEURISTIC_REANCHORED' || reasonCode === 'HEURISTIC_NO_DWARF'
}

export function resolveSourceBreakpoints(
  sourcePath: string,
  lines: number[],
  exportedFunctions: Set<string>,
  previousFunctionMap?: Map<number, string>
): ResolvedBreakpoint[] {
  const ranges = parseFunctionRanges(sourcePath)

  return lines.map((line) => {
    const range = ranges.find(
      (candidate) => line >= candidate.startLine && line <= candidate.endLine
    )
    if (!range) {
      if (previousFunctionMap) {
        const prevFn = previousFunctionMap.get(line)
        if (prevFn) {
          const fnRange = ranges.find((r) => r.name === prevFn)
          if (fnRange) {
            if (!exportedFunctions.has(prevFn)) {
              return {
                requestedLine: line,
                line: fnRange.startLine,
                verified: false,
                functionName: prevFn,
                reasonCode: 'HEURISTIC_NOT_EXPORTED',
                setBreakpoint: shouldPromoteToFunctionBreakpoint(false, prevFn, 'HEURISTIC_NOT_EXPORTED'),
                message: `Rust function '${prevFn}' is not an exported contract entrypoint`,
              }
            }
            return {
              requestedLine: line,
              line: fnRange.startLine,
              verified: false,
              functionName: prevFn,
              reasonCode: 'HEURISTIC_REANCHORED',
              setBreakpoint: shouldPromoteToFunctionBreakpoint(false, prevFn, 'HEURISTIC_REANCHORED'),
              message: `Breakpoint re-anchored to '${prevFn}' after source edit (was line ${line}, now line ${fnRange.startLine})`,
            }
          }
        }
      }
      return {
        requestedLine: line,
        line,
        verified: false,
        reasonCode: 'HEURISTIC_NO_FUNCTION',
        setBreakpoint: shouldPromoteToFunctionBreakpoint(false, undefined, 'HEURISTIC_NO_FUNCTION'),
        message: 'Line is not inside a detectable Rust function',
      }
    }

    if (!exportedFunctions.has(range.name)) {
      return {
        requestedLine: line,
        line,
        verified: false,
        functionName: range.name,
        reasonCode: 'HEURISTIC_NOT_EXPORTED',
        setBreakpoint: shouldPromoteToFunctionBreakpoint(false, range.name, 'HEURISTIC_NOT_EXPORTED'),
        message: `Rust function '${range.name}' is not an exported contract entrypoint`,
      }
    }

    return {
      requestedLine: line,
      line,
      verified: false,
      functionName: range.name,
      reasonCode: 'HEURISTIC_NO_DWARF',
      setBreakpoint: shouldPromoteToFunctionBreakpoint(false, range.name, 'HEURISTIC_NO_DWARF'),
      message: `Heuristic mapping to contract entrypoint '${range.name}' (DWARF source map unavailable)`,
    }
  })
}

export function diagnoseBreakpoints(
  sourcePath: string,
  lines: number[]
): DiagnosticReport[] {
  const ranges = parseFunctionRanges(sourcePath)

  return lines.map((line) => {
    const range = ranges.find(
      (candidate) => line >= candidate.startLine && line <= candidate.endLine
    )
    if (!range) {
      return {
        line,
        status: '❌ Unverified / Ignored',
        reason: 'Line is not inside a detectable Rust function block.',
      }
    }
    return {
      line,
      status: '⚠️ Heuristic Mapping',
      functionName: range.name,
      reason: `DWARF mapping is unavailable. The debugger will attempt to bind to the exported entrypoint '${range.name}'.`,
    }
  })
}
