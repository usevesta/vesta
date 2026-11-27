import type { CSSProperties } from "react"
import { Toaster as Sonner, type ToasterProps } from "sonner"

import { Icon } from "@/components/Icon"
import { useResolvedColorScheme } from "@/theme/theme-sync"

const Toaster = ({ ...props }: ToasterProps) => {
  const theme = useResolvedColorScheme()

  return (
    <Sonner
      theme={theme}
      className="toaster group"
      icons={{
        success: <Icon name="success" className="size-5" />,
        info: <Icon name="info" className="size-5" />,
        warning: <Icon name="warning" className="size-5" />,
        error: <Icon name="error" className="size-5" />,
        loading: <Icon name="loading" className="size-5 motion-safe:animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
