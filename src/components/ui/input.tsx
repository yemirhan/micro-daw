import * as React from "react"

import { cn } from "@/lib/utils"

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "file:text-foreground placeholder:text-muted-foreground selection:bg-primary/40 selection:text-foreground border-border bg-surface-0 h-8 w-full min-w-0 rounded-md border px-2.5 py-1 text-[13px] shadow-[inset_0_1px_2px_oklch(0_0_0/0.3)] transition-[color,border-color,box-shadow] duration-150 hover:border-input outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 ",
        "focus-visible:border-ring focus-visible:ring-ring/30 focus-visible:ring-2",
        "aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        className
      )}
      {...props}
    />
  )
}

export { Input }
