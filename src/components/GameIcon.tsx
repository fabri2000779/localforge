import { useState } from 'react';

interface GameIconProps {
  icon: string;
  logoUrl?: string;
  name: string;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  className?: string;
}

const sizeClasses = {
  sm: 'w-8 h-8 text-2xl',
  md: 'w-10 h-10 text-3xl',
  lg: 'w-12 h-12 text-4xl',
  xl: 'w-16 h-16 text-5xl',
};

export function GameIcon({ icon, logoUrl, name, size = 'md', className = '' }: GameIconProps) {
  const [imageError, setImageError] = useState(false);
  
  const sizeClass = sizeClasses[size];
  
  // If we have a logo URL and it hasn't errored, show the image
  if (logoUrl && !imageError) {
    return (
      <img 
        src={logoUrl} 
        alt={name}
        className={`${sizeClass} object-cover rounded-lg ${className}`}
        onError={() => setImageError(true)}
      />
    );
  }
  
  // Fallback to emoji
  return (
    <span className={`${sizeClass} flex items-center justify-center ${className}`}>
      {icon}
    </span>
  );
}
