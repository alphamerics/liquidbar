import type { TaskbarApp } from "../../lib/shell";

interface AppIconProps {
  app: TaskbarApp;
  color: string;
}

export function AppIcon({ app, color }: AppIconProps) {
  if (app.icon) {
    return <img src={app.icon} alt="" />;
  }

  return (
    <span style={{ background: color }} aria-hidden="true">
      {app.processName.slice(0, 2).toUpperCase()}
    </span>
  );
}
