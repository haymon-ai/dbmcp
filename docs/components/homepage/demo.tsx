import { basePath } from '@/lib/shared';

export function Demo() {
  return (
    <section className="w-full bg-black py-8 md:py-12">
      <div className="mx-auto max-w-[900px]">
        <img
          src={`${basePath}/demo.gif`}
          alt="Demo showing natural-language database queries answered instantly"
          className="w-full rounded-sm aspect-[9/5]"
        />
      </div>
    </section>
  );
}
