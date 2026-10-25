import type { ComponentProps, ReactNode } from "react";

interface IconButtonProps extends ComponentProps<"button"> {
  children: ReactNode;
}

export function IconButton({ children, className = "", ...props }: IconButtonProps) {
  return (
    <button className={`taskbar-button icon-button ${className}`.trim()} {...props}>
      {children}
    </button>
  );
}
