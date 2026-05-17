import { useState } from 'react';

interface GameIconProps {
  icon: string;
  logoUrl?: string;
  name: string;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  className?: string;
}

const sizeClasses = {
  sm: 'w-8 h-8 text-xl',
  md: 'w-12 h-12 text-2xl',
  lg: 'w-14 h-14 text-3xl',
  xl: 'w-20 h-20 text-4xl',
};

/**
 * Render a game's icon — either the configured logo URL (object-contain
 * inside a tinted square so different aspect-ratio logos don't crop or
 * stretch) or the emoji fallback.
 */
export function GameIcon({ icon, logoUrl, name, size = 'md', className = '' }: GameIconProps) {
  const [imageError, setImageError] = useState(false);
  const sizeClass = sizeClasses[size];

  if (logoUrl && !imageError) {
    return (
      <div
        className={`${sizeClass} flex items-center justify-center rounded-lg bg-slate-800/60 p-1.5 shrink-0 ${className}`}
      >
        <img
          src={logoUrl}
          alt={name}
          className="max-w-full max-h-full object-contain"
          loading="lazy"
          onError={() => setImageError(true)}
        />
      </div>
    );
  }

  return (
    <div
      className={`${sizeClass} flex items-center justify-center rounded-lg bg-slate-800/60 shrink-0 ${className}`}
    >
      <span className="leading-none">{icon}</span>
    </div>
  );
}
