"""Valide tous les modules JSON contre les schémas. python tests/validate_modules.py"""
import json, glob, sys, jsonschema
from referencing import Registry, Resource
names = ['expr', 'disease', 'drug', 'patient', 'auscultation', 'inhalant', 'anesthesia_model', 'procedure']
reg = Registry().with_resources([(f'{n}.schema.json', Resource.from_contents(json.load(open(f'schema/{n}.schema.json')))) for n in names])
bad = 0
for pat, sch in [('modules/diseases/*.json', 'disease'), ('modules/drugs/*.json', 'drug'), ('modules/healthy_dog.json', 'patient'), ('modules/auscultation.json', 'auscultation'),
                 ('modules/anesthesia/agents/*.json', 'drug'), ('modules/anesthesia/inhalants/*.json', 'inhalant'),
                 ('modules/anesthesia/anesthesia_model.json', 'anesthesia_model'), ('modules/anesthesia/procedures/*.json', 'procedure')]:
    v = jsonschema.Draft202012Validator(json.load(open(f'schema/{sch}.schema.json')), registry=reg)
    for f in glob.glob(pat):
        errs = list(v.iter_errors(json.load(open(f))))
        for e in errs: print(f, e.message[:200]); bad += 1
print('OK' if not bad else f'{bad} erreurs'); sys.exit(1 if bad else 0)
