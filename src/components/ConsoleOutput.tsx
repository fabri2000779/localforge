// ANSI color code parser for console output

import React from 'react';

// ANSI color codes to Tailwind classes
const ANSI_COLORS: Record<number, string> = {
  // Standard colors (foreground)
  30: 'text-zinc-900',
  31: 'text-red-500',
  32: 'text-green-500',
  33: 'text-yellow-500',
  34: 'text-blue-500',
  35: 'text-purple-500',
  36: 'text-cyan-500',
  37: 'text-zinc-300',
  
  // Bright colors (foreground)
  90: 'text-zinc-500',
  91: 'text-red-400',
  92: 'text-green-400',
  93: 'text-yellow-300',
  94: 'text-blue-400',
  95: 'text-purple-400',
  96: 'text-cyan-400',
  97: 'text-white',
  
  // Background colors
  40: 'bg-zinc-900',
  41: 'bg-red-900',
  42: 'bg-green-900',
  43: 'bg-yellow-900',
  44: 'bg-blue-900',
  45: 'bg-purple-900',
  46: 'bg-cyan-900',
  47: 'bg-zinc-700',
};

interface TextSegment {
  text: string;
  classes: string[];
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

// Parse ANSI escape sequences
function parseAnsi(input: string): TextSegment[] {
  const segments: TextSegment[] = [];
  
  // Match ANSI escape sequences
  const ansiRegex = /\x1b\[([0-9;]*)m|\u001b\[([0-9;]*)m/g;
  
  let lastIndex = 0;
  let currentClasses: string[] = [];
  let bold = false;
  let dim = false;
  let italic = false;
  let underline = false;
  
  let match;
  while ((match = ansiRegex.exec(input)) !== null) {
    if (match.index > lastIndex) {
      const text = input.slice(lastIndex, match.index);
      if (text) {
        segments.push({ text, classes: [...currentClasses], bold, dim, italic, underline });
      }
    }
    
    const codes = (match[1] || match[2] || '0').split(';').map(Number);
    
    for (const code of codes) {
      if (code === 0) {
        currentClasses = [];
        bold = false;
        dim = false;
        italic = false;
        underline = false;
      } else if (code === 1) {
        bold = true;
      } else if (code === 2) {
        dim = true;
      } else if (code === 3) {
        italic = true;
      } else if (code === 4) {
        underline = true;
      } else if (code === 22) {
        bold = false;
        dim = false;
      } else if (code === 23) {
        italic = false;
      } else if (code === 24) {
        underline = false;
      } else if (ANSI_COLORS[code]) {
        if ((code >= 30 && code <= 37) || (code >= 90 && code <= 97)) {
          currentClasses = currentClasses.filter(c => !c.startsWith('text-'));
        } else if (code >= 40 && code <= 47) {
          currentClasses = currentClasses.filter(c => !c.startsWith('bg-'));
        }
        currentClasses.push(ANSI_COLORS[code]);
      }
    }
    
    lastIndex = match.index + match[0].length;
  }
  
  if (lastIndex < input.length) {
    const text = input.slice(lastIndex);
    if (text) {
      segments.push({ text, classes: [...currentClasses], bold, dim, italic, underline });
    }
  }
  
  return segments.length > 0 ? segments : [{ text: input, classes: [] }];
}

// Detect log level and return appropriate styling
function getLogLevelStyle(line: string): { className: string; lineClass: string } {
  const lowerLine = line.toLowerCase();
  
  if (/\berror\b/.test(lowerLine) || /\[error\]/.test(lowerLine) || /exception/i.test(line)) {
    return { className: 'text-red-400', lineClass: 'bg-red-950/20 border-l-red-500' };
  }
  if (/\bwarn(ing)?\b/.test(lowerLine) || /\[warn(ing)?\]/.test(lowerLine)) {
    return { className: 'text-yellow-400', lineClass: 'bg-yellow-950/10 border-l-yellow-500' };
  }
  if (/\bdebug\b/.test(lowerLine) || /\[debug\]/.test(lowerLine)) {
    return { className: 'text-zinc-500', lineClass: '' };
  }
  
  return { className: '', lineClass: '' };
}

interface ConsoleLineProps {
  line: string;
  index: number;
}

export function ConsoleLine({ line, index }: ConsoleLineProps) {
  const hasAnsi = /\x1b\[|\u001b\[/.test(line);
  
  let segments: TextSegment[];
  let lineClass = '';
  
  if (hasAnsi) {
    segments = parseAnsi(line);
  } else {
    const style = getLogLevelStyle(line);
    lineClass = style.lineClass;
    segments = [{ text: line, classes: style.className ? [style.className] : [] }];
  }
  
  // Check if this is a user command echo
  const isCommand = line.startsWith('> ');
  if (isCommand) {
    lineClass = 'bg-indigo-950/20 border-l-indigo-500';
  }
  
  return (
    <div className={`console-line group flex ${lineClass}`}>
      <span className="text-zinc-600 text-xs w-12 flex-shrink-0 select-none pr-2 text-right opacity-50 group-hover:opacity-100">
        {index + 1}
      </span>
      <span className="flex-1 text-zinc-300">
        {segments.map((segment, i) => {
          const style: React.CSSProperties = {};
          if (segment.bold) style.fontWeight = 'bold';
          if (segment.dim) style.opacity = 0.6;
          if (segment.italic) style.fontStyle = 'italic';
          if (segment.underline) style.textDecoration = 'underline';
          
          return (
            <span 
              key={i} 
              className={segment.classes.join(' ')}
              style={Object.keys(style).length > 0 ? style : undefined}
            >
              {segment.text}
            </span>
          );
        })}
      </span>
    </div>
  );
}

interface ConsoleOutputProps {
  logs: string[];
  consoleRef: React.RefObject<HTMLDivElement | null>;
  consoleEndRef: React.RefObject<HTMLDivElement | null>;
  onScroll: () => void;
  emptyMessage?: string;
}

export function ConsoleOutput({ 
  logs, 
  consoleRef, 
  consoleEndRef, 
  onScroll,
  emptyMessage = 'No logs yet'
}: ConsoleOutputProps) {
  return (
    <div 
      ref={consoleRef} 
      onScroll={onScroll} 
      className="console"
    >
      {logs.length === 0 ? (
        <div className="text-zinc-500 p-4 text-center">{emptyMessage}</div>
      ) : (
        <>
          {logs.map((line, i) => (
            <ConsoleLine key={i} line={line} index={i} />
          ))}
          <div ref={consoleEndRef} />
        </>
      )}
    </div>
  );
}
