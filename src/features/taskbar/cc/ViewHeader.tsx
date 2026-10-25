import { ChevronLeft } from "lucide-react";

/** Header row for sub-views: back to the main panel plus the view title. */
export function ViewHeader({
  title,
  onBack,
  className,
}: {
  title: string;
  onBack: () => void;
  className?: string;
}) {
  return (
    <div className={`cc-view-header${className ? ` ${className}` : ""}`}>
      <button type="button" className="cc-back cc-hit" onClick={onBack}>
        <ChevronLeft size={18} />
      </button>
      <span className="cc-view-title">{title}</span>
    </div>
  );
}
