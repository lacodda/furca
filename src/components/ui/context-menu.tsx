import { ContextMenu as Base } from '@base-ui/react/context-menu'
import { menuItemVariants, menuPopupVariants } from './menu'
import { cn } from 'dowel-ui'
import { usePopupContainer } from './layer'

/*
 * ContextMenu.
 *
 * The same list of actions as Menu, opened the other way round: by right
 * click, or by a long press on a touch screen, over an *area* rather than
 * from a button. So the trigger is not a control - it is the region the menu
 * belongs to, a row, a canvas, a file tile - and it renders a `<div>`.
 *
 * That difference is the whole component. Everything below the Root is Menu's
 * own: Base UI re-exports the Portal, Positioner, Popup, Item and the rest
 * from the menu package, so the popup that opens here is literally the same
 * popup, with the same keyboard, the same type-ahead and the same submenus.
 *
 * Which is why the clothes are imported rather than copied. Two `cva` calls
 * that started identical drift within a release - one gets a padding fix, the
 * other does not - and then a product has two menus that are almost the same,
 * which is worse than two that differ on purpose.
 *
 * What Base UI does *not* give this one is `openOnHover`, `modal` or a
 * `handle`: a context menu is opened by a gesture over an anchor point, not
 * by a trigger element it can be attached to from elsewhere.
 */

/** The root. It positions against the pointer, so there is nothing to anchor
 * and nothing to control but `onOpenChange`. */
export const ContextMenu = Base.Root

/** The area that opens it. A `<div>`, not a button: give it `render` to make
 * it the row or the canvas it belongs to rather than a wrapper. */
export const ContextMenuTrigger = Base.Trigger

/** A labelled group of items, for a menu long enough to need headings. */
export const ContextMenuGroup = Base.Group

/** A submenu, opened from a `ContextMenuSubTrigger` inside the parent. */
export const ContextMenuSub = Base.SubmenuRoot

export interface ContextMenuPopupProps extends Base.Popup.Props {
  /** How wide the popup starts. The same three as Menu's. */
  size?: 'sm' | 'md' | 'lg'
  /** Where to portal to. Defaults to the raised host of the overlay this is
   * opened inside (`layer.tsx`), and to the document body when there is none -
   * either way not the element it was opened from, whose `overflow` would clip
   * it. Pass an element to put it somewhere else, such as a container being
   * screenshotted. */
  container?: Base.Portal.Props['container']
}

/** The panel. Portalled, and positioned against the point that was clicked
 * rather than against an element. */
export function ContextMenuPopup({
  size,
  container,
  className,
  children,
  ...props
}: ContextMenuPopupProps) {
  // Inside an overlay, the overlay's raised host rather than the body - or
  // this popup draws under the dialog, drawer or popover that opened it. See
  // `layer.tsx`. Outside every overlay the hook gives `undefined`: the body.
  const host = usePopupContainer()

  return (
    <Base.Portal container={container ?? host}>
      <Base.Positioner className="[z-index:var(--z-menu)]">
        <Base.Popup className={cn(menuPopupVariants({ size }), className)} {...props}>
          {children}
        </Base.Popup>
      </Base.Positioner>
    </Base.Portal>
  )
}

export interface ContextMenuItemProps extends Base.Item.Props {
  /** `danger` draws the destructive one apart, in the colour of something
   * that cannot be undone. */
  tone?: 'default' | 'danger'
}

/** An action. */
export function ContextMenuItem({ tone, className, ...props }: ContextMenuItemProps) {
  return <Base.Item className={cn(menuItemVariants({ tone }), className)} {...props} />
}

/** An item that opens a submenu. Drawn like any other, because it is one. */
export function ContextMenuSubTrigger({ tone, className, ...props }: ContextMenuItemProps) {
  return <Base.SubmenuTrigger className={cn(menuItemVariants({ tone }), className)} {...props} />
}

export interface ContextMenuCheckboxItemProps extends Base.CheckboxItem.Props {
  /** `danger` draws the destructive one apart. */
  tone?: 'default' | 'danger'
}

/** An item that carries a tick. The state is the caller's.
 *
 * Typed from `CheckboxItem`, not `Item`: declared with the plain item's props,
 * it rendered a checkbox that refused `checked` and `onCheckedChange` - the
 * two things it is for. Menu's had the same fault until kilna's column
 * picker called it; this one waited for a test to. */
export function ContextMenuCheckboxItem({ tone, className, ...props }: ContextMenuCheckboxItemProps) {
  return <Base.CheckboxItem className={cn(menuItemVariants({ tone }), className)} {...props} />
}

/** The tick itself, drawn only when the item is checked. */
export const ContextMenuCheckboxIndicator = Base.CheckboxItemIndicator

/** A line between groups of items. Decorative, and marked as such. */
export function ContextMenuSeparator({ className, ...props }: Base.Separator.Props) {
  return <Base.Separator className={cn('-mx-1 my-1 h-px bg-line', className)} {...props} />
}

/** The caption above a group. */
export function ContextMenuGroupLabel({ className, ...props }: Base.GroupLabel.Props) {
  return (
    <Base.GroupLabel
      className={cn('caption px-2 py-1.5', className)}
      {...props}
    />
  )
}
