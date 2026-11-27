import { useId, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { setLanguage } from '@/i18n'
import { isLanguage, SUPPORTED_LANGUAGES } from '@/i18n/language'
import type { IconName } from '@/lib/icons'
import { COLOR_SCHEME_PREFERENCES, presetSupportsSchemeChoice, THEME_PRESETS } from '@/theme/theme'
import { selectPreset, useThemeStore } from '@/theme/theme-store'

interface Choice<T extends string> {
  value: T
  label: string
  icon?: IconName
}

function ChoiceGroup<T extends string>({
  label,
  value,
  choices,
  onChange,
}: {
  label: string
  /** null: none of the choices is selected. */
  value: T | null
  choices: readonly Choice<T>[]
  onChange: (value: T) => void
}) {
  return (
    <div role="group" aria-label={label} className="flex gap-2">
      {choices.map((choice) => (
        <Button
          key={choice.value}
          variant={choice.value === value ? 'default' : 'secondary'}
          aria-pressed={choice.value === value}
          className="h-auto min-h-14 flex-1 gap-2 text-base"
          onClick={() => {
            onChange(choice.value)
          }}
        >
          {choice.icon !== undefined && <Icon name={choice.icon} className="size-5" />}
          {choice.label}
        </Button>
      ))}
    </div>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-sm font-medium text-muted-foreground">{title}</h3>
      {children}
    </section>
  )
}

/** Per-screen display choices: style, light/dark, transparency, language. */
export function DisplaySettings() {
  const { t, i18n } = useTranslation()
  const { presetOverride, colorScheme, glass, setPresetOverride, setColorScheme, setGlass } = useThemeStore()
  const preset = useThemeStore(selectPreset)
  const glassSwitchId = useId()
  const current = i18n.resolvedLanguage ?? i18n.language
  const language = isLanguage(current) ? current : 'en'

  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button variant="ghost" className="size-14 rounded-xl" aria-label={t('display.open')}>
          <Icon name="settings" className="size-7" />
        </Button>
      </DialogTrigger>
      <DialogContent className="gap-6 p-6 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="text-2xl">{t('display.title')}</DialogTitle>
          <DialogDescription className="text-base">{t('display.description')}</DialogDescription>
        </DialogHeader>

        <Section title={t('display.style')}>
          <Button
            variant={presetOverride === null ? 'default' : 'secondary'}
            aria-pressed={presetOverride === null}
            className="h-auto min-h-14 w-full gap-2 text-base"
            onClick={() => {
              setPresetOverride(null)
            }}
          >
            <Icon name="home" className="size-5" />
            {t('display.homeStyle')}
          </Button>
          <ChoiceGroup
            label={t('display.style')}
            value={presetOverride}
            onChange={setPresetOverride}
            choices={THEME_PRESETS.map((value) => ({ value, label: t(`display.preset.${value}`) }))}
          />
        </Section>

        {presetSupportsSchemeChoice(preset) && (
          <Section title={t('display.colorScheme')}>
            <ChoiceGroup
              label={t('display.colorScheme')}
              value={colorScheme}
              onChange={setColorScheme}
              choices={COLOR_SCHEME_PREFERENCES.map((value) => ({
                value,
                label: t(`display.scheme.${value}`),
                icon: `scheme_${value}` as const,
              }))}
            />
          </Section>
        )}

        {preset === 'glass-dark' && (
          <label htmlFor={glassSwitchId} className="flex min-h-14 cursor-pointer items-center gap-4">
            <span className="flex flex-1 flex-col">
              <span className="text-base font-medium">{t('display.effects')}</span>
              <span className="text-sm text-muted-foreground">{t('display.effectsHint')}</span>
            </span>
            <Switch id={glassSwitchId} size="lg" checked={glass} onCheckedChange={setGlass} />
          </label>
        )}

        <Section title={t('display.language')}>
          <ChoiceGroup
            label={t('display.language')}
            value={language}
            onChange={setLanguage}
            choices={SUPPORTED_LANGUAGES.map((value) => ({ value, label: t(`language.${value}`) }))}
          />
        </Section>
      </DialogContent>
    </Dialog>
  )
}
