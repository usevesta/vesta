import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import type { IconName } from '@/lib/icons'
import type { WidgetStatusProps } from '@/widgets/types'
import { WidgetFrame } from '@/widgets/WidgetFrame'

export interface CameraPreviewProps extends WidgetStatusProps {
  /** e.g. "Front door camera". */
  label: string
  /** Semantic icon. Default "camera". */
  icon?: IconName
}

/** Placeholder until live video (WebRTC) lands. */
export function CameraPreview({ label, icon = 'camera', pending, unavailable }: CameraPreviewProps) {
  const { t } = useTranslation()
  return (
    <WidgetFrame icon={icon} title={label} pending={pending} unavailable={unavailable}>
      <div className="flex min-h-32 flex-1 flex-col items-center justify-center gap-3 rounded-xl bg-muted p-4 text-center text-muted-foreground">
        <Icon name="video_off" className="size-10" />
        <p className="text-base">{t('camera.notAvailable')}</p>
      </div>
    </WidgetFrame>
  )
}
