import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { ProductMark } from '@/components/ui/product-mark'
import { Splash as SplashScreen } from '@/components/ui/splash'

/**
 * What the window shows while it opens the repositories it had open.
 *
 * The same picture `index.html` painted before the bundle arrived - the mark,
 * the name, the promise, the version - with the same geometry, so nothing
 * jumps when React takes over, plus the line only the application can say:
 * which repository it is reading. The static picture is taken down on the
 * first render, because from this moment the page is the one drawing.
 */
export function Splash({ status }: { status: string }) {
  const { t } = useTranslation()

  useEffect(() => {
    document.getElementById('splash')?.remove()
  }, [])

  return (
    <SplashScreen
      mark={<ProductMark product="furca" size={56} />}
      name={t('app.name')}
      tagline={t('app.tagline')}
      version={`v${__APP_VERSION__}`}
      status={status}
      tip={t('splash.tip')}
    />
  )
}
