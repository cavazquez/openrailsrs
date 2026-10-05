"""Published URLs, accessibility essentials and reproducible static generation."""
from html.parser import HTMLParser
from pathlib import Path
import json
import unittest
from urllib.parse import urlsplit, unquote
from build_website import ROOT, SITE, render, content_catalogue

class Document(HTMLParser):
    def __init__(self,text):
        super().__init__();self.links=[];self.ids=[];self.h1=0;self.images=[];self.lang=None
        self.feed(text)
    def handle_starttag(self,tag,attrs):
        a=dict(attrs)
        if tag=='html':self.lang=a.get('lang')
        if tag=='h1':self.h1+=1
        if 'id' in a:self.ids.append(a['id'])
        if tag=='img':self.images.append(a)
        for key in ['href','src']:
            if key in a:self.links.append(a[key])

class WebsiteTests(unittest.TestCase):
    def test_generated_and_published_pages_have_valid_links_and_accessible_content(self):
        pages=render()
        documents={p.name:Document(text) for p,text in pages.items()}
        self.assertEqual(len(pages),len(json.loads((SITE/'site.json').read_text())['pages']))
        for p,text in pages.items():
            with self.subTest(page=p.name):
                self.assertEqual(p.read_text(),text,'generated HTML is stale')
                self.assertEqual((ROOT/'docs'/p.name).read_text(),text,'published HTML is stale')
                doc=documents[p.name]
                self.assertEqual(doc.h1,1);self.assertEqual(doc.lang,'es')
                self.assertEqual(len(doc.ids),len(set(doc.ids)))
                self.assertIn('contenido',doc.ids)
                for image in doc.images:
                    self.assertIn('alt',image)
                    self.assertTrue(image['alt'] or image.get('aria-hidden')=='true')
                for link in doc.links:
                    url=urlsplit(link)
                    if url.scheme or url.netloc:continue
                    self.assertFalse(url.path.startswith('/'),'Pages project must use relative asset paths')
                    target=SITE/unquote(url.path) if url.path else p
                    self.assertTrue(target.is_file(),link)
                    if url.fragment and target.suffix=='.html':self.assertIn(unquote(url.fragment),documents[target.name].ids,link)
        for path in ['css/style.css','js/site.js','assets/mark.svg','assets/exterior.webp','assets/cabina.webp','assets/nieve.webp','assets/noche.webp']:
            self.assertEqual((SITE/path).read_bytes(),(ROOT/'docs'/path).read_bytes())

    def test_capture_provenance_and_site_versions_are_explicit(self):
        provenance=json.loads((SITE/'assets/provenance.json').read_text())
        self.assertTrue(provenance)
        text=''.join(render().values())
        self.assertNotIn('{{',text)
        self.assertIn('1.6.1',text)
        self.assertIn('0.19.1',text)
        self.assertIn('Belgrano',text)
        self.assertNotIn('cdn.jsdelivr',text)

    def test_catalogue_preserves_every_original_source_without_download_mirrors(self):
        catalogue = json.loads((ROOT/'docs/fixtures/content/official-catalog.json').read_text())
        cards = Document(content_catalogue())
        self.assertEqual(len(cards.ids), len(catalogue['routes']))
        expected = [route['url'].removesuffix('.git') for route in catalogue['routes']
                    if route['url'].startswith('https://')]
        self.assertCountEqual(cards.links, expected)
        self.assertNotIn('download=', content_catalogue())

if __name__=='__main__':unittest.main()
