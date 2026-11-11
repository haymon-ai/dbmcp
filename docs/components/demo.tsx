import { basePath } from '@/lib/shared';

/** Demo recording (a VHS GIF from docs/tapes) on the navy code panel. */
export function Demo({
  src = '/demo.gif',
  alt = 'Demo showing natural-language database queries answered instantly',
}: {
  src?: string;
  alt?: string;
}) {
  return (
    <div className="not-prose mb-8 overflow-hidden rounded-[3px] bg-code">
      <img src={`${basePath}${src}`} alt={alt} className="block aspect-[9/5] w-full" />
    </div>
  );
}
