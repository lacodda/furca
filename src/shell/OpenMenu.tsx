import { useTranslation } from 'react-i18next'
import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import {
  Menu,
  MenuGroup,
  MenuGroupLabel,
  MenuItem,
  MenuPopup,
  MenuSeparator,
  MenuSub,
  MenuSubTrigger,
  MenuTrigger,
} from '@/components/ui/menu'
import type { Line } from '@/lib/generated/Line'
import type { WindowState } from '@/lib/generated/WindowState'

export interface OpenActions {
  /** Ask for a folder and open the repository in it. */
  browse: () => void
  open: (path: string) => void
  openSession: (name: string) => void
  openLine: () => void
  /** Ask for a name and save the open tabs under it. */
  saveSession: () => void
  deleteSession: (name: string) => void
}

/** How many of the recent repositories the menu lists. */
const RECENT_SHOWN = 8

/**
 * The `+` after the tabs: everything that opens repositories, in one place.
 *
 * Open a folder, one opened lately, or a set of them - a session saved under
 * a name, or the line, which is every repository the owner's record keeps
 * and is read fresh each time rather than saved, so a project added to the
 * record is in it the next time it opens.
 */
export function OpenMenu({
  state,
  line,
  actions,
}: {
  state: WindowState
  line: Line | null
  actions: OpenActions
}) {
  const { t } = useTranslation()
  const open = new Set(state.tabs.map((tab) => tab.path))
  const recent = state.recent.filter((tab) => !open.has(tab.path)).slice(0, RECENT_SHOWN)

  return (
    <Menu>
      <MenuTrigger
        render={
          <Button
            variant="icon"
            size="icon-sm"
            className="mx-1 self-center"
            aria-label={t('open.menu')}
            title={t('open.menu')}
          />
        }
      >
        <Plus aria-hidden />
      </MenuTrigger>
      <MenuPopup align="start" className="min-w-64">
        <MenuItem onClick={actions.browse}>
          {t('open.browse')}
          <Kbd keys="Mod+O" aria-hidden className="ml-auto" />
        </MenuItem>

        {recent.length > 0 && (
          <>
            <MenuSeparator />
            <MenuGroup>
              <MenuGroupLabel>{t('open.recent')}</MenuGroupLabel>
              {recent.map((tab) => (
                <MenuItem key={tab.path} onClick={() => actions.open(tab.path)} title={tab.path}>
                  <span className="truncate">{tab.name}</span>
                </MenuItem>
              ))}
            </MenuGroup>
          </>
        )}

        <MenuSeparator />
        <MenuGroup>
          <MenuGroupLabel>{t('open.sessions')}</MenuGroupLabel>
          {line?.state === 'kept' && (
            <MenuItem onClick={actions.openLine} title={t('open.lineHint')}>
              {t('open.line')}
              <span className="ml-auto font-mono text-2xs text-faint">{line.tabs.length}</span>
            </MenuItem>
          )}
          {state.sessions.map((session) => (
            <MenuItem key={session.name} onClick={() => actions.openSession(session.name)}>
              <span className="truncate">{session.name}</span>
              <span className="ml-auto font-mono text-2xs text-faint">{session.tabs.length}</span>
            </MenuItem>
          ))}
          <MenuItem onClick={actions.saveSession} disabled={state.tabs.length === 0}>
            {t('open.saveSession')}
          </MenuItem>
          {state.sessions.length > 0 && (
            <MenuSub>
              <MenuSubTrigger>{t('open.forgetSession')}</MenuSubTrigger>
              <MenuPopup>
                {state.sessions.map((session) => (
                  <MenuItem
                    key={session.name}
                    tone="danger"
                    onClick={() => actions.deleteSession(session.name)}
                  >
                    {session.name}
                  </MenuItem>
                ))}
              </MenuPopup>
            </MenuSub>
          )}
        </MenuGroup>
      </MenuPopup>
    </Menu>
  )
}
