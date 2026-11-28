import { useId, useState, type ReactNode, type SubmitEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Icon } from '@/components/Icon'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { MAX_LABEL_CHARS, MAX_TITLE_CHARS } from '@/editor/model'
import { isPageIcon, PAGE_ICONS, type PageIcon } from '@/editor/page-icons'

interface NameFormProps {
  title: string
  description?: string
  label: string
  initial: string
  placeholder?: string
  maxLength: number
  /** Can the name be left empty? */
  allowEmpty: boolean
  submitLabel: string
  onSubmit: (name: string) => void
  onCancel: () => void
  children?: ReactNode
}

/** Big text field with Cancel / Save, submitted with the keyboard's Enter too. */
function NameForm({
  title,
  description,
  label,
  initial,
  placeholder,
  maxLength,
  allowEmpty,
  submitLabel,
  onSubmit,
  onCancel,
  children,
}: NameFormProps) {
  const { t } = useTranslation()
  const inputId = useId()
  const [name, setName] = useState(initial)
  const canSubmit = allowEmpty || name.trim() !== ''

  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (canSubmit) onSubmit(name.trim())
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-6">
      <DialogHeader>
        <DialogTitle className="text-2xl">{title}</DialogTitle>
        {description !== undefined && <DialogDescription className="text-base">{description}</DialogDescription>}
      </DialogHeader>
      <div className="flex flex-col gap-2">
        <label htmlFor={inputId} className="text-sm font-medium text-muted-foreground">
          {label}
        </label>
        <Input
          id={inputId}
          value={name}
          placeholder={placeholder}
          maxLength={maxLength}
          autoComplete="off"
          enterKeyHint="done"
          className="h-14 rounded-xl px-4 text-lg md:text-lg"
          onChange={(event) => {
            setName(event.target.value)
          }}
        />
      </div>
      {children}
      <div className="flex gap-3">
        <Button type="button" variant="outline" className="h-auto min-h-14 flex-1 text-lg" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" className="h-auto min-h-14 flex-1 text-lg" disabled={!canSubmit}>
          {submitLabel}
        </Button>
      </div>
    </form>
  )
}

export interface TileNameDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The custom name the tile has now ("" for none). */
  label: string
  /** The name it shows without a custom one; undefined when it shows none (the clock). */
  ownName: string | undefined
  onRename: (label: string) => void
}

/** Rename a tile. Empty goes back to the thing's own name. */
export function TileNameDialog({ open, onOpenChange, label, ownName, onRename }: TileNameDialogProps) {
  const { t } = useTranslation()
  const close = () => {
    onOpenChange(false)
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-6 p-6 sm:max-w-lg" showCloseButton={false}>
        {open && (
          <NameForm
            title={t('editor.rename.title')}
            description={
              ownName === undefined ? t('editor.rename.hintNoName') : t('editor.rename.hint', { name: ownName })
            }
            label={t('editor.rename.label')}
            initial={label}
            placeholder={ownName}
            maxLength={MAX_LABEL_CHARS}
            allowEmpty
            submitLabel={t('editor.rename.save')}
            onCancel={close}
            onSubmit={(name) => {
              onRename(name)
              close()
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

export interface PageNameDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  mode: 'add' | 'rename'
  title: string
  icon: string | undefined
  onSubmit: (title: string, icon: PageIcon) => void
}

/** Mounted each time the dialog opens, so it starts from the page's own icon. */
function PageNameForm({ mode, title, icon, onSubmit, onClose }: Omit<PageNameDialogProps, 'open' | 'onOpenChange'> & { onClose: () => void }) {
  const { t } = useTranslation()
  const [chosen, setChosen] = useState<PageIcon>(isPageIcon(icon) ? icon : 'home')
  const iconLabelId = useId()
  return (
    <NameForm
      title={mode === 'add' ? t('editor.pageName.addTitle') : t('editor.pageName.renameTitle')}
      label={t('editor.pageName.label')}
      initial={title}
      placeholder={t('editor.pageName.defaultName')}
      maxLength={MAX_TITLE_CHARS}
      allowEmpty={mode === 'add'}
      submitLabel={mode === 'add' ? t('editor.pageName.add') : t('editor.pageName.save')}
      onCancel={onClose}
      onSubmit={(name) => {
        onSubmit(name === '' ? t('editor.pageName.defaultName') : name, chosen)
        onClose()
      }}
    >
      <div className="flex flex-col gap-2">
        <span id={iconLabelId} className="text-sm font-medium text-muted-foreground">
          {t('editor.pageName.icon')}
        </span>
        <div role="radiogroup" aria-labelledby={iconLabelId} className="grid grid-cols-6 gap-2">
          {PAGE_ICONS.map((name) => (
            <Button
              key={name}
              type="button"
              role="radio"
              aria-checked={chosen === name}
              aria-label={t(`editor.pageIcons.${name}`)}
              variant={chosen === name ? 'default' : 'secondary'}
              className="h-14 w-full rounded-xl"
              onClick={() => {
                setChosen(name)
              }}
            >
              <Icon name={name} className="size-7" />
            </Button>
          ))}
        </div>
      </div>
    </NameForm>
  )
}

/** Name and icon of a new page, or of one being renamed. */
export function PageNameDialog({ open, onOpenChange, ...form }: PageNameDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-6 p-6 sm:max-w-lg" showCloseButton={false}>
        {open && (
          <PageNameForm
            {...form}
            onClose={() => {
              onOpenChange(false)
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}
